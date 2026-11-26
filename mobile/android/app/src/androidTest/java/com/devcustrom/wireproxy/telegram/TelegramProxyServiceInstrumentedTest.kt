package com.devcustrom.wireproxy.telegram

import android.content.Intent
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Starts the real foreground service, checks the exposed tg:// link and relays
 * a request through the service listener.
 */
@RunWith(AndroidJUnit4::class)
class TelegramProxyServiceInstrumentedTest {
    @Test
    fun serviceStartsAndRelays() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        TelegramSettings.init(context)
        try {
            context.startActivity(
                Intent()
                    .setClassName(
                        context.packageName,
                        "com.devcustrom.wireproxy.debug.DebugVpnActivity",
                    )
                    .putExtra("telegram", true)
                    .putExtra("dpi", true)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            )
            val deadline = System.currentTimeMillis() + 15_000
            while (System.currentTimeMillis() < deadline && !TelegramState.status.value.running) {
                Thread.sleep(250)
            }
            val status = TelegramState.status.value
            assertTrue("service did not start: ${status.message}", status.running)
            assertTrue(
                "unexpected link: ${status.link}",
                status.link.startsWith("tg://proxy?server=127.0.0.1&port=1443&secret=dd"),
            )
            org.junit.Assert.assertEquals(
                "secret must be 32 hex chars",
                32,
                status.link.substringAfter("secret=dd").length,
            )
            val constructor = MtProtoTestClient.reqPq(1443, TelegramSettings.secret())
            assertTrue(
                "expected resPQ, got 0x${constructor.toString(16)} route=${status.route}",
                MtProtoTestClient.isResPq(constructor),
            )
        } finally {
            TelegramProxyService.stop(context)
            Thread.sleep(500)
        }
    }
}
