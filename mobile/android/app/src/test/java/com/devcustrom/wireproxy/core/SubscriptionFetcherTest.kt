package com.devcustrom.wireproxy.core

import java.util.Base64
import org.junit.Assert.assertEquals
import org.junit.Test

class SubscriptionFetcherTest {
    private val plain = """
        # comment
        vless://9d8c3007-dfcd-4d34-a729-ae793f826581@example.com:443#One
        ss://YWVzLTI1Ni1nY206cGFzcw==@1.2.3.4:8388#Two
    """.trimIndent()

    @Test
    fun parsesPlainAndBase64() {
        assertEquals(2, SubscriptionFetcher.links(plain).size)
        val encoded = Base64.getEncoder().encodeToString(plain.toByteArray())
        assertEquals(2, SubscriptionFetcher.links(encoded).size)
    }

    @Test
    fun ignoresOtherSchemes() {
        assertEquals(0, SubscriptionFetcher.links("https://example.com/sub").size)
    }
}
