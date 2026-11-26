package com.devcustrom.wireproxy.core

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/**
 * Non-secret preferences. Secrets live in the Android Keystore stores.
 */
object AppSettings {
    private const val PREFS = "wireproxy_settings"
    private var prefs: android.content.SharedPreferences? = null

    var dpiEnabled by mutableStateOf(true)
        private set
    var dpiMode by mutableStateOf("medium")
        private set
    var directDomains by mutableStateOf("")
        private set
    var excludedPackages by mutableStateOf<Set<String>>(emptySet())
        private set

    fun init(context: Context) {
        if (prefs != null) return
        prefs = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val store = prefs ?: return
        dpiEnabled = store.getBoolean("dpi", true)
        dpiMode = store.getString("dpi_mode", "medium") ?: "medium"
        directDomains = store.getString("direct_domains", "") ?: ""
        excludedPackages = (store.getString("excluded_packages", "") ?: "")
            .split('\n')
            .map(String::trim)
            .filter(String::isNotEmpty)
            .toSet()
    }

    fun updateDpi(enabled: Boolean) {
        dpiEnabled = enabled
        persist()
    }

    fun updateDpiMode(mode: String) {
        if (mode !in setOf("light", "medium", "aggressive")) return
        dpiMode = mode
        persist()
    }

    fun updateDirectDomains(value: String) {
        directDomains = value
        persist()
    }

    fun updateExcludedPackages(packages: Set<String>) {
        excludedPackages = packages
        persist()
    }

    fun directDomainList(): List<String> = directDomains
        .split('\n', ',', ';', ' ')
        .map(String::trim)
        .filter { it.isNotEmpty() && !it.startsWith("#") }
        .distinct()
        .take(300)

    private fun persist() {
        prefs?.edit()
            ?.putBoolean("dpi", dpiEnabled)
            ?.putString("dpi_mode", dpiMode)
            ?.putString("direct_domains", directDomains)
            ?.putString("excluded_packages", excludedPackages.joinToString("\n"))
            ?.apply()
    }
}
