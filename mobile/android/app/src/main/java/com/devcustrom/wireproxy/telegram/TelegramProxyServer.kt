package com.devcustrom.wireproxy.telegram

import android.util.Log
import java.io.IOException
import java.io.InputStream
import java.net.InetSocketAddress
import java.net.Proxy
import java.net.ServerSocket
import java.net.Socket
import java.util.Collections
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelChildren
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * Local MTProto proxy that relays Telegram's obfuscated2 transport over
 * Telegram's WebSocket gateways (`/apiws`), with a raw TCP fallback.
 *
 * Only loopback is exposed: the user opens a `tg://proxy?server=127.0.0.1`
 * link on the same device.
 */
class TelegramProxyServer(
    private val secret: ByteArray,
    private val port: Int = 1443,
    private val byedpiPort: Int? = null,
    private val onStats: (clients: Int, up: Long, down: Long, route: String) -> Unit,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val clients = Collections.synchronizedSet(mutableSetOf<Socket>())
    private val activeCount = AtomicInteger(0)
    private val bytesUp = AtomicLong(0)
    private val bytesDown = AtomicLong(0)

    @Volatile
    private var lastRoute: String? = null

    @Volatile
    private var server: ServerSocket? = null

    @Volatile
    private var stopped = false

    private var statsJob: Job? = null

    fun start(): Boolean {
        val socket = runCatching {
            ServerSocket().apply {
                reuseAddress = true
                bind(InetSocketAddress("127.0.0.1", port), 64)
            }
        }.getOrElse {
            Log.e(TAG, "cannot bind port $port", it)
            return false
        }
        server = socket
        scope.launch { acceptLoop(socket) }
        scope.launch { TelegramRoutes.refresh() }
        statsJob = scope.launch {
            while (isActive && !stopped) {
                publish()
                kotlinx.coroutines.delay(2_000)
            }
        }
        return true
    }

    fun stop() {
        stopped = true
        runCatching { server?.close() }
        server = null
        synchronized(clients) {
            clients.forEach { runCatching { it.close() } }
            clients.clear()
        }
        statsJob?.cancel()
        scope.cancel()
        onStats(0, bytesUp.get(), bytesDown.get(), "")
    }

    private suspend fun acceptLoop(socket: ServerSocket) {
        while (!stopped) {
            val client = try {
                socket.accept()
            } catch (error: IOException) {
                if (!stopped) Log.d(TAG, "accept stopped: ${error.message}")
                break
            }
            clients += client
            if (activeCount.get() >= MAX_CLIENTS) {
                clients -= client
                runCatching { client.close() }
                continue
            }
            scope.launch { handleClient(client) }
        }
    }

    private suspend fun handleClient(socket: Socket) {
        val remote = socket.remoteSocketAddress?.toString() ?: "?"
        activeCount.incrementAndGet()
        publish()
        try {
            socket.tcpNoDelay = true
            socket.soTimeout = HANDSHAKE_TIMEOUT_MS
            val input = socket.getInputStream()
            val output = socket.getOutputStream()
            val handshake = readExactly(input, MtProtoCrypto.HANDSHAKE_LEN) ?: return
            val init = MtProtoCrypto.parseClientInit(handshake, secret)
            if (init == null) {
                Log.w(TAG, "bad handshake from $remote")
                return
            }
            socket.soTimeout = 0
            var dc = init.dc
            if (dc >= 10_000) dc -= 10_000
            if (dc <= 0) return
            val signedDc = if (init.media) -dc else dc
            val relayInit = MtProtoCrypto.generateRelayInit(init.protocol, signedDc)
            val crypto = MtProtoCrypto.buildCrypto(init, secret, relayInit)
            Log.i(
                TAG,
                "client $remote DC$dc${if (init.media) " media" else ""} " +
                    "proto=${MtProtoCrypto.protocolName(init.protocol)}",
            )
            val upstream = openUpstream(dc, init.media, relayInit) ?: run {
                Log.w(TAG, "no upstream for DC$dc")
                return
            }
            try {
                bridge(socket, input, output, upstream, crypto, init.protocol)
            } finally {
                upstream.close()
            }
        } catch (error: Throwable) {
            if (error is kotlinx.coroutines.CancellationException) {
                Log.d(TAG, "client cancelled")
            } else if (error is IOException) {
                Log.d(TAG, "client closed: ${error.message}")
            } else {
                Log.e(TAG, "client failed", error)
            }
        } finally {
            clients -= socket
            runCatching { socket.close() }
            activeCount.decrementAndGet()
            publish()
        }
    }

    private suspend fun openUpstream(
        dc: Int,
        media: Boolean,
        relayInit: ByteArray,
    ): Upstream? {
        val routes = wsRoutes(dc, media)
        val winner = connectAnyWs(routes)
        if (winner != null) {
            val (session, host) = winner
            if (session.send(relayInit)) {
                lastRoute = if (host.endsWith(".web.telegram.org")) {
                    "ws:$host"
                } else {
                    "cf:$host"
                }
                Log.i(TAG, "upstream via $host")
                return WsUpstream(session)
            }
            session.close()
        }
        val ip = dcIp(dc)
        val tcp = connectTcp(ip)
        if (tcp != null) {
            tcp.outputStream.write(relayInit)
            tcp.outputStream.flush()
            lastRoute = "tcp:$ip"
            Log.i(TAG, "upstream via tcp $ip")
            return TcpUpstream(tcp)
        }
        return null
    }

    private fun wsRoutes(dc: Int, media: Boolean): List<Pair<String, String>> {
        val hostDc = if (dc == 203) 2 else dc
        val names = LinkedHashSet<String>()
        lastRoute
            ?.takeIf { it.startsWith("ws:") || it.startsWith("cf:") }
            ?.substringAfter(':')
            ?.takeIf { host ->
                host.startsWith("kws$hostDc.") || host.startsWith("kws$hostDc-")
            }
            ?.let { names += it }
        if (media) names += "kws$hostDc-1.web.telegram.org"
        names += "kws$hostDc.web.telegram.org"
        TelegramRoutes.domains.take(4).forEach { names += "kws$hostDc.$it" }
        return names.map { host -> "wss://$host/apiws" to host }
    }

    private suspend fun connectAnyWs(
        routes: List<Pair<String, String>>,
    ): Pair<TelegramWebSocket, String>? {
        if (routes.isEmpty()) return null
        val result = CompletableDeferred<Pair<TelegramWebSocket, String>?>()
        val remaining = AtomicInteger(routes.size)
        val raceScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
        routes.forEach { (url, host) ->
            raceScope.launch {
                val ws = runCatching {
                    // The WS edge is pinned to 149.154.167.220. Routing it
                    // through a SOCKS proxy would pass the blocked hostname and
                    // make ByeDPI resolve it, so DPI applies to the raw TCP
                    // fallback instead.
                    TelegramWebSocket.connect(url, WEBSOCKET_TIMEOUT_SECONDS, null)
                }.onFailure {
                    Log.d(TAG, "route $host failed: ${it.message}")
                }.getOrNull()
                if (ws != null) {
                    if (result.isCompleted) {
                        ws.close()
                    } else {
                        result.complete(ws to host)
                    }
                } else if (remaining.decrementAndGet() == 0 && !result.isCompleted) {
                    result.complete(null)
                }
            }
        }
        val winner = result.await()
        raceScope.cancel()
        return winner
    }

    private fun connectTcp(ip: String): Socket? {
        val proxyPort = byedpiPort
        if (proxyPort != null) {
            val proxied = runCatching {
                Socket(
                    Proxy(
                        Proxy.Type.SOCKS,
                        InetSocketAddress("127.0.0.1", proxyPort),
                    ),
                ).apply {
                    connect(InetSocketAddress(ip, DC_PORT), TCP_TIMEOUT_MS)
                    soTimeout = 0
                    tcpNoDelay = true
                }
            }.getOrNull()
            if (proxied != null) return proxied
        }
        return runCatching {
            Socket().apply {
                connect(InetSocketAddress(ip, DC_PORT), TCP_TIMEOUT_MS)
                soTimeout = 0
                tcpNoDelay = true
            }
        }.getOrNull()
    }

    private suspend fun bridge(
        socket: Socket,
        input: InputStream,
        output: java.io.OutputStream,
        upstream: Upstream,
        crypto: MtProtoCrypto.Crypto,
        protocol: Int,
    ) {
        val splitter = MsgSplitter(protocol)
        val finished = CompletableDeferred<Unit>()
        coroutineScope {
            launch(Dispatchers.IO) {
                try {
                    val buffer = ByteArray(64 * 1024)
                    while (isActive) {
                        val read = input.read(buffer)
                        if (read < 0) break
                        if (read == 0) continue
                        val chunk = buffer.copyOf(read)
                        val plain = crypto.clientDecrypt.update(chunk)
                        val relayCipher = crypto.relayEncrypt.update(plain)
                        bytesUp.addAndGet(read.toLong())
                        val parts = splitter.split(relayCipher, plain)
                        for (part in parts) {
                            if (!upstream.send(part)) {
                                throw IOException("upstream send failed")
                            }
                        }
                    }
                    splitter.flush()?.let { upstream.send(it) }
                } catch (error: Throwable) {
                    if (error !is IOException) Log.d(TAG, "up stream ended", error)
                } finally {
                    finished.complete(Unit)
                }
            }
            launch(Dispatchers.IO) {
                try {
                    while (isActive) {
                        val data = upstream.receive() ?: break
                        val plain = crypto.relayDecrypt.update(data)
                        val toClient = crypto.clientEncrypt.update(plain)
                        output.write(toClient)
                        output.flush()
                        bytesDown.addAndGet(data.size.toLong())
                    }
                } catch (error: Throwable) {
                    if (error !is IOException) Log.d(TAG, "down stream ended", error)
                } finally {
                    finished.complete(Unit)
                }
            }
            finished.await()
            runCatching { socket.close() }
            upstream.close()
            coroutineContext.cancelChildren()
        }
    }

    private fun publish() {
        onStats(activeCount.get(), bytesUp.get(), bytesDown.get(), lastRoute.orEmpty())
    }

    private fun readExactly(input: InputStream, size: Int): ByteArray? {
        val buffer = ByteArray(size)
        var offset = 0
        while (offset < size) {
            val read = input.read(buffer, offset, size - offset)
            if (read < 0) return null
            offset += read
        }
        return buffer
    }

    private fun dcIp(dc: Int): String = when (dc) {
        // Raw MTProto TCP endpoints. The .220 edge used by the WS pool does
        // not accept an obfuscated2 handshake outside a WebSocket.
        1 -> "149.154.175.50"
        2 -> "149.154.167.51"
        3 -> "149.154.175.100"
        4 -> "149.154.167.91"
        5 -> "149.154.171.5"
        203 -> "91.105.192.100"
        else -> "149.154.167.51"
    }

    private interface Upstream {
        suspend fun send(data: ByteArray): Boolean
        suspend fun receive(): ByteArray?
        fun close()
    }

    private class WsUpstream(
        private val socket: TelegramWebSocket,
    ) : Upstream {
        override suspend fun send(data: ByteArray): Boolean = socket.send(data)
        override suspend fun receive(): ByteArray? = socket.receive()
        override fun close() = socket.close()
    }

    private class TcpUpstream(
        private val socket: Socket,
    ) : Upstream {
        private val input = socket.getInputStream()
        private val output = socket.getOutputStream()

        override suspend fun send(data: ByteArray): Boolean = runCatching {
            output.write(data)
            output.flush()
            true
        }.getOrDefault(false)

        override suspend fun receive(): ByteArray? {
            val buffer = ByteArray(64 * 1024)
            val read = runCatching { input.read(buffer) }.getOrDefault(-1)
            return if (read <= 0) null else buffer.copyOf(read)
        }

        override fun close() {
            runCatching { socket.close() }
        }
    }

    companion object {
        private const val TAG = "TelegramProxy"
        private const val DC_PORT = 443
        private const val TCP_TIMEOUT_MS = 6_000
        private const val HANDSHAKE_TIMEOUT_MS = 15_000
        private const val WEBSOCKET_TIMEOUT_SECONDS = 6L
        private const val MAX_CLIENTS = 64
    }
}
