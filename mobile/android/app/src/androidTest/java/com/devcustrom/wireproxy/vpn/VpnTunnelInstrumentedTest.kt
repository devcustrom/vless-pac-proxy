package com.devcustrom.wireproxy.vpn

import android.os.ParcelFileDescriptor
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.devcustrom.wireproxy.core.TunnelState
import com.devcustrom.wireproxy.core.VpnConfigBuilder
import java.io.FileInputStream
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Brings up the real VpnService with a direct outbound and checks that another
 * UID (the shell) can carry TCP through the TUN.
 */
@RunWith(AndroidJUnit4::class)
class VpnTunnelInstrumentedTest {
    @Test
    fun directTunnelCarriesTcp() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val automation = instrumentation.uiAutomation

        shell(automation, "appops set ${context.packageName} ACTIVATE_VPN allow")
        shell(
            automation,
            "pm grant ${context.packageName} android.permission.POST_NOTIFICATIONS",
        )
        val prepare = android.net.VpnService.prepare(context)
        org.junit.Assert.assertNull("VPN consent is missing: $prepare", prepare)

        val outbounds = JSONArray().put(
            JSONObject()
                .put("tag", "smoke-direct")
                .put("protocol", "freedom")
                .put("settings", JSONObject()),
        )
        val config = VpnConfigBuilder.build(outbounds, dpi = false)
        try {
            WireproxyVpnService.start(context, config)
            waitForTunnel()
            val route = shell(automation, "ip route get 1.1.1.1")
            assertTrue("traffic does not use tun0: $route", route.contains("tun0"))
        } finally {
            WireproxyVpnService.stop(context)
            Thread.sleep(500)
        }
    }

    private fun waitForTunnel() {
        val deadline = System.currentTimeMillis() + 20_000
        while (System.currentTimeMillis() < deadline) {
            val status = TunnelState.status.value
            if (status.running) return
            if (status.message.contains("не ") || status.message.contains("Не ")) {
                throw AssertionError("VPN failed: ${status.message}")
            }
            Thread.sleep(250)
        }
        throw AssertionError("VPN did not start: ${TunnelState.status.value.message}")
    }

    private fun shell(automation: android.app.UiAutomation, command: String): String {
        val descriptor: ParcelFileDescriptor = automation.executeShellCommand(command)
        return descriptor.use {
            FileInputStream(it.fileDescriptor).bufferedReader().readText()
        }
    }
}
