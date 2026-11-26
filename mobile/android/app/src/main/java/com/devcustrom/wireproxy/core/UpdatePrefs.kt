package com.devcustrom.wireproxy.core

import android.content.Context

/** Non-secret update bookkeeping. */
object UpdatePrefs {
    private const val PREFS = "wireproxy_updates"
    private const val LAST_CHECK = "last_check"
    private const val NOTIFIED = "notified_version"
    private var prefs: android.content.SharedPreferences? = null

    fun init(context: Context) {
        if (prefs == null) {
            prefs = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        }
    }

    fun lastCheck(): Long = prefs?.getLong(LAST_CHECK, 0L) ?: 0L

    fun setLastCheck(context: Context) {
        init(context)
        prefs?.edit()?.putLong(LAST_CHECK, System.currentTimeMillis())?.apply()
    }

    fun notifiedVersion(): String? = prefs?.getString(NOTIFIED, null)

    fun setNotifiedVersion(context: Context, version: String) {
        init(context)
        prefs?.edit()?.putString(NOTIFIED, version)?.apply()
    }
}
