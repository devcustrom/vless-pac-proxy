package com.devcustrom.wireproxy.core

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

data class TunnelStatus(
    val running: Boolean = false,
    val message: String = "VPN выключен",
    val exitHint: String = "",
)

object TunnelState {
    private val mutable = MutableStateFlow(TunnelStatus())
    val status: StateFlow<TunnelStatus> = mutable.asStateFlow()

    fun update(status: TunnelStatus) {
        mutable.value = status
    }

    fun running(message: String = "VPN включён") {
        mutable.value = TunnelStatus(true, message)
    }

    fun stopped(message: String = "VPN выключен") {
        mutable.value = TunnelStatus(false, message)
    }

    fun error(message: String) {
        mutable.value = TunnelStatus(false, message)
    }
}
