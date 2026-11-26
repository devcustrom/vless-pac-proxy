package com.devcustrom.wireproxy

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import com.devcustrom.wireproxy.core.AppSettings
import com.devcustrom.wireproxy.core.ImportBus
import com.devcustrom.wireproxy.core.ProfileStore
import com.devcustrom.wireproxy.core.SubscriptionManager
import com.devcustrom.wireproxy.core.SubscriptionStore
import com.devcustrom.wireproxy.core.UpdatePrefs
import com.devcustrom.wireproxy.core.UpdateScheduler
import com.devcustrom.wireproxy.telegram.TelegramSettings
import com.devcustrom.wireproxy.ui.WireproxyApp
import com.devcustrom.wireproxy.ui.theme.WireproxyTheme

class MainActivity : ComponentActivity() {
    private var vpnPrepared by mutableStateOf(false)
    private var openUpdates by mutableStateOf(false)

    private val vpnPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
            vpnPrepared = result.resultCode == RESULT_OK
        }

    private val notificationPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        openUpdates = intent?.getBooleanExtra("open_updates", false) == true
        handleIntent(intent)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        AppSettings.init(this)
        ProfileStore.init(this)
        SubscriptionStore.init(this)
        TelegramSettings.init(this)
        SubscriptionManager.schedule(this)
        SubscriptionManager.syncNow(this)
        UpdatePrefs.init(this)
        UpdateScheduler.schedule(this)
        setContent {
            WireproxyTheme {
                WireproxyApp(
                    vpnPermissionGranted = vpnPrepared,
                    onRequestVpnPermission = ::requestVpnPermission,
                    openUpdates = openUpdates,
                    onUpdatesOpened = { openUpdates = false },
                )
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        openUpdates = intent?.getBooleanExtra("open_updates", false) == true
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        if (intent == null) return
        val text = when (intent.action) {
            Intent.ACTION_SEND -> intent.getStringExtra(Intent.EXTRA_TEXT)
            Intent.ACTION_VIEW -> intent.dataString
            else -> null
        }
        if (!text.isNullOrBlank()) {
            ImportBus.submit(text)
        }
    }

    private fun requestVpnPermission() {
        val intent: Intent? = VpnService.prepare(this)
        if (intent == null) {
            vpnPrepared = true
        } else {
            vpnPermissionLauncher.launch(intent)
        }
    }
}
