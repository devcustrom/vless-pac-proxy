package com.devcustrom.wireproxy.telegram

import java.util.concurrent.atomic.AtomicReference
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * End-to-end check against the real Telegram DC: performs the obfuscated2
 * client handshake through the local proxy and sends an unencrypted
 * `req_pq_multi`. A `resPQ` response proves both relay directions work.
 *
 * Runs on a device/emulator with network access.
 */
class TelegramProxyInstrumentedTest {
    @Test
    fun relaysMediaDc4() {
        val secret = ByteArray(16) { (it * 5 + 9).toByte() }
        val port = 15444
        val stats = AtomicReference<TelegramStatus>()
        val server = TelegramProxyServer(
            secret = secret,
            port = port,
            byedpiPort = null,
        ) { clients, up, down, route ->
            stats.set(
                TelegramStatus(
                    running = true,
                    clients = clients,
                    bytesUp = up,
                    bytesDown = down,
                    route = route,
                ),
            )
        }
        assertTrue("proxy did not start", server.start())
        try {
            val constructor = MtProtoTestClient.reqPq(
                port = port,
                secret = secret,
                dc = 4,
                media = true,
            )
            assertTrue(
                "expected media resPQ, got 0x${constructor.toString(16)} " +
                    "route=${stats.get()?.route}",
                MtProtoTestClient.isResPq(constructor),
            )
        } finally {
            server.stop()
        }
    }

    @Test
    fun relaysReqPqToTelegram() {
        val secret = ByteArray(16) { (it * 7 + 3).toByte() }
        val port = 15443
        val stats = AtomicReference<TelegramStatus>()
        val server = TelegramProxyServer(
            secret = secret,
            port = port,
            byedpiPort = null,
        ) { clients, up, down, route ->
            stats.set(
                TelegramStatus(
                    running = true,
                    clients = clients,
                    bytesUp = up,
                    bytesDown = down,
                    route = route,
                ),
            )
        }
        assertTrue("proxy did not start", server.start())
        try {
            val constructor = MtProtoTestClient.reqPq(port, secret)
            assertTrue(
                "expected resPQ, got 0x${constructor.toString(16)} " +
                    "route=${stats.get()?.route}",
                MtProtoTestClient.isResPq(constructor),
            )
        } finally {
            server.stop()
        }
    }
}
