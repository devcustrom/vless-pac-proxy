package com.devcustrom.wireproxy.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.os.ParcelFileDescriptor
import androidx.core.content.ContextCompat
import com.devcustrom.wireproxy.MainActivity
import com.devcustrom.wireproxy.core.AppSettings
import com.devcustrom.wireproxy.core.TunnelState
import com.devcustrom.wireproxy.core.XrayBridge
import hev.htproxy.TProxyService
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

class WireproxyVpnService : VpnService() {
    private var tun: ParcelFileDescriptor? = null
    private val worker = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override fun onCreate() {
        super.onCreate()
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL,
                "VPN-подключение",
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                worker.launch { stopTunnel("VPN выключен") }
                return START_NOT_STICKY
            }
            ACTION_START -> {
                val config = intent.getStringExtra(EXTRA_CONFIG)
                if (config.isNullOrBlank()) {
                    TunnelState.error("Нет конфигурации VPN")
                    stopSelf()
                    return START_NOT_STICKY
                }
                startForeground(NOTIFICATION_ID, notification("Подключаю VPN…"))
                // Xray/TProxy startup is blocking; never run it on the main thread.
                worker.launch {
                    startTunnel(config)
                }
            }
        }
        return START_NOT_STICKY
    }

    private fun startTunnel(config: String) {
        stopCore()
        AppSettings.init(this)
        val builder = Builder()
            .setSession("Wireproxy")
            .setMtu(MTU)
            .addAddress("10.111.222.1", 32)
            .addRoute("0.0.0.0", 0)
            .addAddress("fd00:111:222::1", 128)
            .addRoute("::", 0)
            .addDnsServer("1.1.1.1")
            .addDnsServer("2606:4700:4700::1111")
            .setBlocking(true)
        // Xray protects its outbound sockets through DialerController.
        AppSettings.excludedPackages
            .filter { it != packageName }
            .forEach { excluded ->
                runCatching { builder.addDisallowedApplication(excluded) }
            }

        val descriptor = runCatching { builder.establish() }.getOrElse {
            TunnelState.error("Не удалось создать TUN: ${it.message}")
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return
        }
        if (descriptor == null) {
            TunnelState.error("Android не выдал TUN (нет разрешения VPN?)")
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return
        }
        tun = descriptor
        XrayBridge.attachVpn(this)
        val tested = XrayBridge.test(config)
        if (!tested.success) {
            closeTun()
            stopCore()
            TunnelState.error(tested.error.ifBlank { "Xray-конфиг не прошёл проверку" })
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return
        }
        val result = XrayBridge.run(config)
        if (!result.success) {
            closeTun()
            stopCore()
            TunnelState.error(result.error.ifBlank { "Xray не запустился" })
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return
        }
        val hevConfig = File(cacheDir, "hev-tunnel.yml").apply {
            writeText(
                """
                tunnel:
                  name: tun0
                  mtu: $MTU
                  ipv4: 10.111.222.1
                  ipv6: 'fd00:111:222::1'
                socks5:
                  address: 127.0.0.1
                  port: $SOCKS_PORT
                  udp: 'udp'
                misc:
                  log-level: warn
                  log-file: ${File(cacheDir, "hev.log").absolutePath}
                """.trimIndent() + "\n",
            )
        }
        if (!TProxyService.TProxyStartService(hevConfig.absolutePath, descriptor.fd)) {
            closeTun()
            stopCore()
            TunnelState.error("hev-socks5-tunnel не запустился")
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return
        }
        TunnelState.running("VPN включён")
        val manager = getSystemService(NotificationManager::class.java)
        manager.notify(NOTIFICATION_ID, notification("VPN включён"))
    }

    private fun stopTunnel(message: String) {
        stopCore()
        closeTun()
        TunnelState.stopped(message)
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun stopCore() {
        runCatching { TProxyService.TProxyStopService() }
        XrayBridge.stop()
        XrayBridge.resetDns()
    }

    private fun closeTun() {
        runCatching { tun?.close() }
        tun = null
    }

    override fun onRevoke() {
        worker.launch { stopTunnel("VPN отключён системой") }
        super.onRevoke()
    }

    override fun onDestroy() {
        runCatching { TProxyService.TProxyStopService() }
        runCatching { XrayBridge.stop() }
        XrayBridge.resetDns()
        closeTun()
        worker.cancel()
        super.onDestroy()
    }

    private fun notification(text: String): Notification {
        val intent = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setContentTitle("Wireproxy")
            .setContentText(text)
            .setContentIntent(intent)
            .setOngoing(true)
            .build()
    }

    companion object {
        private const val CHANNEL = "wireproxy_vpn"
        private const val NOTIFICATION_ID = 1080
        private const val MTU = 1500
        private const val SOCKS_PORT = 25346
        private const val ACTION_START = "com.devcustrom.wireproxy.START_VPN"
        private const val ACTION_STOP = "com.devcustrom.wireproxy.STOP_VPN"
        private const val EXTRA_CONFIG = "config"

        fun start(context: Context, config: String) {
            val intent = Intent(context, WireproxyVpnService::class.java)
                .setAction(ACTION_START)
                .putExtra(EXTRA_CONFIG, config)
            ContextCompat.startForegroundService(context, intent)
        }

        fun stop(context: Context) {
            context.startService(
                Intent(context, WireproxyVpnService::class.java).setAction(ACTION_STOP),
            )
        }
    }
}
