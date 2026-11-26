package com.devcustrom.wireproxy.core

import androidx.test.platform.app.InstrumentationRegistry
import java.net.InetSocketAddress
import java.net.Proxy
import java.util.concurrent.TimeUnit
import okhttp3.OkHttpClient
import okhttp3.Request
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Verifies that the packaged ciadpi executable can be launched from
 * nativeLibraryDir and that its SOCKS5 listener can reach the network.
 */
class ByeDpiInstrumentedTest {
    @Test
    fun startsAndProxiesHttps() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        assertTrue("ByeDPI did not start: ${ByeDpi.lastError}", ByeDpi.start(context, "light"))
        try {
            assertTrue(ByeDpi.running)
            val client = OkHttpClient.Builder()
                .proxy(Proxy(Proxy.Type.SOCKS, InetSocketAddress("127.0.0.1", ByeDpi.PORT)))
                .connectTimeout(15, TimeUnit.SECONDS)
                .readTimeout(15, TimeUnit.SECONDS)
                .build()
            val response = client.newCall(
                Request.Builder().url("https://cp.cloudflare.com/").build(),
            ).execute()
            response.use {
                assertTrue("HTTP ${it.code}", it.isSuccessful)
            }
        } finally {
            ByeDpi.stop()
        }
    }
}
