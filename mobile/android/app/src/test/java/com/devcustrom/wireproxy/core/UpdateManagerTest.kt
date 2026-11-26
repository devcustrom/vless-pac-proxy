package com.devcustrom.wireproxy.core

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdateManagerTest {
    @Test
    fun comparesVersionsNumerically() {
        assertTrue(UpdateManager.isNewer("0.7.0", "0.6.13"))
        assertTrue(UpdateManager.isNewer("0.6.14", "0.6.13"))
        assertFalse(UpdateManager.isNewer("0.6.13", "0.6.13"))
        assertFalse(UpdateManager.isNewer("0.6.9", "0.6.13"))
        assertTrue(UpdateManager.isNewer("v1.0.0", "0.9.9"))
    }
}
