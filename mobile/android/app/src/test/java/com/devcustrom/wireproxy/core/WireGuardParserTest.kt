package com.devcustrom.wireproxy.core

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WireGuardParserTest {
    @Test
    fun recognizesWireGuardConfig() {
        assertTrue(
            WireGuardParser.isWireGuard(
                """
                [Interface]
                PrivateKey = key
                [Peer]
                PublicKey = key
                Endpoint = vpn.example.com:51820
                """.trimIndent(),
            ),
        )
        assertFalse(WireGuardParser.isWireGuard("vless://example"))
    }
}
