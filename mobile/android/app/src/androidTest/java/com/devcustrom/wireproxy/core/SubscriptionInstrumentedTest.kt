package com.devcustrom.wireproxy.core

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.BufferedReader
import java.io.InputStreamReader
import java.net.InetAddress
import java.net.ServerSocket
import kotlin.concurrent.thread
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Exercises subscription fetch, share-link conversion, encrypted persistence
 * and replacement of the previous profile set. Uses a loopback HTTP server so
 * the test needs no external subscription.
 */
@RunWith(AndroidJUnit4::class)
class SubscriptionInstrumentedTest {
    private val link =
        "vless://11111111-2222-3333-4444-555555555555@example.com:443" +
            "?encryption=none&security=tls&sni=example.com&type=ws&path=%2F#TestNode"

    @Test
    fun addsRefreshesAndRemovesSubscription() = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        SubscriptionStore.init(context)
        ProfileStore.init(context)
        val server = ServerSocket(0, 1, InetAddress.getByName("127.0.0.1"))
        val body = link.toByteArray()
        val serving = thread {
            while (!server.isClosed) {
                runCatching {
                    server.accept().use { socket ->
                        val reader = BufferedReader(InputStreamReader(socket.getInputStream()))
                        while (true) {
                            val line = reader.readLine() ?: break
                            if (line.isEmpty()) break
                        }
                        val header = "HTTP/1.1 200 OK\r\n" +
                            "Content-Type: text/plain\r\n" +
                            "Content-Length: ${body.size}\r\n" +
                            "Connection: close\r\n\r\n"
                        socket.getOutputStream().apply {
                            write(header.toByteArray())
                            write(body)
                            flush()
                        }
                    }
                }
            }
        }
        val url = "http://127.0.0.1:${server.localPort}/sub"
        try {
            val added = SubscriptionManager.add(context, url)
            assertEquals("add error: ${added.error}", 1, added.added)
            val subscription = SubscriptionStore.items.first { it.url == url }
            assertNotNull(subscription)
            assertEquals(1, ProfileStore.items.count { it.sourceId == subscription.id })
            assertTrue(ProfileStore.items.any { it.name == "TestNode" })

            val refreshed = SubscriptionManager.refresh(context, subscription.id)
            assertEquals("refresh error: ${refreshed.error}", 1, refreshed.added)
            assertEquals(1, ProfileStore.items.count { it.sourceId == subscription.id })

            SubscriptionManager.remove(context, subscription.id)
            assertEquals(0, ProfileStore.items.count { it.sourceId == subscription.id })
            assertTrue(SubscriptionStore.items.none { it.id == subscription.id })
        } finally {
            serving.join(2_000)
            runCatching { server.close() }
        }
    }
}
