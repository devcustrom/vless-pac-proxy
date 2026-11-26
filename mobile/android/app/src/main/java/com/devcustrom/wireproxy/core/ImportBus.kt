package com.devcustrom.wireproxy.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/**
 * Carries shared/opened links from the activity into the Compose screen.
 * The screen clears the value after importing it.
 */
object ImportBus {
    var pending by mutableStateOf<String?>(null)
        private set

    fun submit(text: String) {
        if (text.isNotBlank()) pending = text
    }

    fun clear() {
        pending = null
    }
}
