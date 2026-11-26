package com.devcustrom.wireproxy.telegram

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

data class TelegramStatus(
    val running: Boolean = false,
    val message: String = "Telegram-прокси выключен",
    val link: String = "",
    val clients: Int = 0,
    val bytesUp: Long = 0,
    val bytesDown: Long = 0,
    val route: String = "",
)

object TelegramState {
    private val mutable = MutableStateFlow(TelegramStatus())
    val status: StateFlow<TelegramStatus> = mutable.asStateFlow()

    fun update(status: TelegramStatus) {
        mutable.value = status
    }

    fun stopped(message: String = "Telegram-прокси выключен") {
        mutable.value = TelegramStatus(false, message)
    }

    fun running(
        message: String,
        link: String,
        clients: Int,
        bytesUp: Long,
        bytesDown: Long,
        route: String,
    ) {
        mutable.value = TelegramStatus(
            running = true,
            message = message,
            link = link,
            clients = clients,
            bytesUp = bytesUp,
            bytesDown = bytesDown,
            route = route,
        )
    }

    fun error(message: String) {
        mutable.value = TelegramStatus(false, message)
    }
}
