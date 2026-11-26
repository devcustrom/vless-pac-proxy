package com.devcustrom.wireproxy.telegram

import java.io.IOException
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Proxy
import java.util.concurrent.CountDownLatch
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import okhttp3.Dns
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import okio.ByteString.Companion.toByteString

/**
 * Blocking facade over OkHttp's WebSocket. The bridge code needs ordered
 * messages with backpressure, not callbacks.
 */
internal class TelegramWebSocket private constructor(
    private val socket: WebSocket,
    private val events: LinkedBlockingQueue<Event>,
) {
    sealed interface Event {
        class Data(val bytes: ByteArray) : Event
        class Closed(val reason: String) : Event
    }

    fun send(bytes: ByteArray): Boolean =
        socket.send(bytes.toByteString())

    /** Returns the next upstream message or null when the socket closed. */
    fun receive(): ByteArray? {
        while (true) {
            val event = events.take()
            when (event) {
                is Event.Data -> if (event.bytes.isNotEmpty()) return event.bytes
                is Event.Closed -> return null
            }
        }
    }

    fun close() {
        events.offer(Event.Closed("client closed"))
        runCatching { socket.close(1000, null) }
        runCatching { socket.cancel() }
    }

    companion object {
        private const val MAX_MESSAGES = 256
        private const val WS_EDGE_IP = "149.154.167.220"

        fun connect(
            url: String,
            timeoutSeconds: Long = 6,
            byedpiPort: Int? = null,
        ): TelegramWebSocket {
            val builder = OkHttpClient.Builder()
                .connectTimeout(timeoutSeconds, TimeUnit.SECONDS)
                .readTimeout(0, TimeUnit.MILLISECONDS)
                .writeTimeout(20, TimeUnit.SECONDS)
                .retryOnConnectionFailure(false)
                // Telegram's edge at 149.154.167.220 fronts DC2/DC4/DC203 and
                // accepts the WS upgrade with the kwsN Host/SNI. Direct DNS for
                // these names is frequently blocked, so pin the edge here.
                .dns(object : Dns {
                    override fun lookup(hostname: String): List<InetAddress> {
                        val pinned = hostname.startsWith("kws2.") ||
                            hostname.startsWith("kws4.") ||
                            hostname.startsWith("kws4-1.")
                        if (pinned && hostname.endsWith(".web.telegram.org")) {
                            return listOf(InetAddress.getByName(WS_EDGE_IP))
                        }
                        return Dns.SYSTEM.lookup(hostname)
                    }
                })
            if (byedpiPort != null) {
                builder.proxy(
                    Proxy(
                        Proxy.Type.SOCKS,
                        InetSocketAddress("127.0.0.1", byedpiPort),
                    ),
                )
            }
            val client = builder.build()
            val events = LinkedBlockingQueue<Event>(MAX_MESSAGES)
            val opened = CountDownLatch(1)
            var failure: Throwable? = null
            var socketRef: WebSocket? = null

            val listener = object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) {
                    opened.countDown()
                }

                override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
                    if (!events.offer(Event.Data(bytes.toByteArray()), 40, TimeUnit.SECONDS)) {
                        webSocket.cancel()
                    }
                }

                override fun onMessage(webSocket: WebSocket, text: String) {
                    webSocket.cancel()
                }

                override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                    runCatching { webSocket.close(1000, null) }
                }

                override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                    events.offer(Event.Closed(reason.ifBlank { "ws closed" }))
                    opened.countDown()
                }

                override fun onFailure(
                    webSocket: WebSocket,
                    error: Throwable,
                    response: Response?,
                ) {
                    failure = error
                    events.offer(Event.Closed(error.message ?: "ws failure"))
                    opened.countDown()
                }
            }

            val request = Request.Builder()
                .url(url)
                .header("Sec-WebSocket-Protocol", "binary")
                .build()
            socketRef = client.newWebSocket(request, listener)
            if (!opened.await(timeoutSeconds, TimeUnit.SECONDS)) {
                socketRef.cancel()
                throw IOException("WebSocket timeout: $url")
            }
            failure?.let {
                socketRef.cancel()
                throw IOException("WebSocket failed: ${it.message}", it)
            }
            return TelegramWebSocket(socketRef, events)
        }
    }
}
