package com.devcustrom.wireproxy.debug

import android.app.Activity
import android.os.Bundle
import com.devcustrom.wireproxy.core.VpnConfigBuilder
import com.devcustrom.wireproxy.core.WireGuardParser
import com.devcustrom.wireproxy.telegram.TelegramProxyService
import com.devcustrom.wireproxy.vpn.WireproxyVpnService
import java.io.File
import org.json.JSONArray
import org.json.JSONObject

/**
 * Debug-only entry point for on-device smoke tests: brings up the VPN with a
 * direct or WireGuard outbound so another UID can verify that TCP really
 * traverses the TUN. Never included in release builds.
 */
class DebugVpnActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val dpi = intent.getBooleanExtra("dpi", false)
        if (intent.getBooleanExtra("telegram", false)) {
            TelegramProxyService.start(this, dpi)
            finish()
            return
        }
        val wgFile = intent.getStringExtra("wgFile")
        val outbounds = JSONArray().put(
            if (!wgFile.isNullOrBlank()) {
                WireGuardParser.buildOutbound(File(wgFile).readText())
            } else {
                JSONObject()
                    .put("tag", "smoke-direct")
                    .put("protocol", "freedom")
                    .put("settings", JSONObject())
            },
        )
        val directDomains = intent.getStringExtra("directDomains")
            ?.split(',', ' ', ';')
            ?.map(String::trim)
            ?.filter(String::isNotEmpty)
            .orEmpty()
        WireproxyVpnService.start(
            this,
            VpnConfigBuilder.build(outbounds, dpi, directDomains = directDomains),
        )
        finish()
    }
}
