package com.devcustrom.wireproxy.telegram

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.IBinder
import androidx.core.content.ContextCompat
import com.devcustrom.wireproxy.MainActivity
import com.devcustrom.wireproxy.core.AppSettings
import com.devcustrom.wireproxy.core.ByeDpi
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/**
 * Foreground service hosting the loopback Telegram MTProto/WSS proxy.
 */
class TelegramProxyService : Service() {
    private var server: TelegramProxyServer? = null
    private var dpiRequested = false
    private val worker = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override fun onCreate() {
        super.onCreate()
        TelegramSettings.init(this)
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL,
                "Telegram-прокси",
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopProxy("Telegram-прокси выключен")
                return START_NOT_STICKY
            }
            else -> {
                dpiRequested = intent?.getBooleanExtra(EXTRA_DPI, dpiRequested) ?: dpiRequested
                startForeground(NOTIFICATION_ID, notification("Запускаю прокси…"))
                startProxy()
            }
        }
        return START_STICKY
    }

    private fun startProxy() {
        if (server != null) return
        worker.launch {
            AppSettings.init(this@TelegramProxyService)
            val useDpi = dpiRequested &&
                ByeDpi.start(this@TelegramProxyService, AppSettings.dpiMode)
            if (dpiRequested && !useDpi) {
                TelegramState.error("ByeDPI не запустился: ${ByeDpi.lastError ?: "неизвестно"}")
            }
            val link = TelegramSettings.link(PORT)
            val instance = TelegramProxyServer(
                secret = TelegramSettings.secret(),
                port = PORT,
                byedpiPort = if (useDpi) ByeDpi.PORT else null,
            ) { clients, up, down, route ->
                val message = if (route.isBlank()) {
                    "Жду подключения Telegram"
                } else {
                    "Подключено: $route"
                }
                TelegramState.running(
                    message = message,
                    link = link,
                    clients = clients,
                    bytesUp = up,
                    bytesDown = down,
                    route = route,
                )
            }
            if (!instance.start()) {
                TelegramState.error("Не удалось занять порт $PORT")
                ByeDpi.stop()
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                return@launch
            }
            server = instance
            TelegramState.running(
                message = "Прокси запущен, жду Telegram",
                link = link,
                clients = 0,
                bytesUp = 0,
                bytesDown = 0,
                route = "",
            )
            val manager = getSystemService(NotificationManager::class.java)
            manager.notify(NOTIFICATION_ID, notification("Прокси запущен · 127.0.0.1:$PORT"))
        }
    }

    private fun stopProxy(message: String) {
        server?.stop()
        server = null
        ByeDpi.stop()
        TelegramState.stopped(message)
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        server?.stop()
        server = null
        ByeDpi.stop()
        TelegramState.stopped()
        worker.cancel()
        super.onDestroy()
    }

    private fun notification(text: String): Notification {
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val stop = PendingIntent.getService(
            this,
            1,
            Intent(this, TelegramProxyService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setContentTitle("Telegram-прокси")
            .setContentText(text)
            .setContentIntent(open)
            .addAction(Notification.Action.Builder(null, "Выключить", stop).build())
            .setOngoing(true)
            .build()
    }

    companion object {
        private const val CHANNEL = "telegram_proxy"
        private const val NOTIFICATION_ID = 1443
        private const val PORT = 1443
        private const val ACTION_START = "com.devcustrom.wireproxy.START_TG"
        private const val ACTION_STOP = "com.devcustrom.wireproxy.STOP_TG"
        private const val EXTRA_DPI = "dpi"

        fun start(context: Context, dpi: Boolean) {
            val intent = Intent(context, TelegramProxyService::class.java)
                .setAction(ACTION_START)
                .putExtra(EXTRA_DPI, dpi)
            ContextCompat.startForegroundService(context, intent)
        }

        fun stop(context: Context) {
            context.startService(
                Intent(context, TelegramProxyService::class.java).setAction(ACTION_STOP),
            )
        }
    }
}
