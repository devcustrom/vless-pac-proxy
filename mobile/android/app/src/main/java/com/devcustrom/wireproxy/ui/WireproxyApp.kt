package com.devcustrom.wireproxy.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.PowerSettingsNew
import androidx.compose.material.icons.filled.Security
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.SwapHoriz
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.devcustrom.wireproxy.BuildConfig
import com.devcustrom.wireproxy.core.AppSettings
import com.devcustrom.wireproxy.core.ImportBus
import com.devcustrom.wireproxy.core.ProfileStore
import com.devcustrom.wireproxy.core.SubscriptionManager
import com.devcustrom.wireproxy.core.SubscriptionStore
import com.devcustrom.wireproxy.core.TunnelState
import com.devcustrom.wireproxy.core.TunnelStatus
import com.devcustrom.wireproxy.core.InstallResult
import com.devcustrom.wireproxy.core.UpdateManager
import com.devcustrom.wireproxy.core.VpnConfigBuilder
import com.devcustrom.wireproxy.core.WireGuardParser
import com.devcustrom.wireproxy.core.XrayBridge
import com.devcustrom.wireproxy.telegram.TelegramProxyService
import com.devcustrom.wireproxy.telegram.TelegramState
import com.devcustrom.wireproxy.telegram.TelegramStatus
import com.devcustrom.wireproxy.vpn.WireproxyVpnService
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import android.content.Intent
import android.net.Uri
import android.provider.Settings

private enum class Tab(val label: String, val icon: ImageVector) {
    Home("Подключение", Icons.Default.Security),
    Profiles("Профили", Icons.AutoMirrored.Filled.List),
    Settings("Настройки", Icons.Default.Settings),
}

private enum class Mode(val label: String) {
    Vpn("VPN"),
    Telegram("Telegram"),
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WireproxyApp(
    vpnPermissionGranted: Boolean,
    onRequestVpnPermission: () -> Unit,
    openUpdates: Boolean = false,
    onUpdatesOpened: () -> Unit = {},
) {
    var tab by rememberSaveable { mutableStateOf(Tab.Home) }
    var mode by rememberSaveable { mutableStateOf(Mode.Vpn) }
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    val tunnel by TunnelState.status.collectAsState()
    val telegram by TelegramState.status.collectAsState()
    val activeRunning = if (mode == Mode.Vpn) tunnel.running else telegram.running

    LaunchedEffect(openUpdates) {
        if (openUpdates) {
            tab = Tab.Settings
            onUpdatesOpened()
            // Notification taps start a fresh process, so the in-memory
            // update state is empty; refresh it instead of showing a blank card.
            scope.launch { UpdateManager.check(context) }
        }
    }

    fun toast(text: String) {
        scope.launch { snackbar.showSnackbar(text) }
    }

    fun importProfile(raw: String) {
        val value = raw.trim()
        if (value.isEmpty()) {
            toast("Вставьте ссылку профиля")
            return
        }
        if (value.length > 1_000_000) {
            toast("Слишком большой профиль")
            return
        }
        scope.launch {
            val isWireGuard = withContext(Dispatchers.IO) { WireGuardParser.isWireGuard(value) }
            val outbounds = if (isWireGuard) {
                runCatching {
                    JSONArray().put(
                        withContext(Dispatchers.IO) { WireGuardParser.buildOutbound(value) },
                    )
                }.getOrElse {
                    toast(it.message ?: "Не удалось разобрать WireGuard")
                    return@launch
                }
            } else {
                val result = withContext(Dispatchers.IO) { XrayBridge.convertShareLinks(value) }
                if (!result.success) {
                    toast(result.error.ifBlank { "Не удалось разобрать профиль" })
                    return@launch
                }
                result.data?.optJSONArray("outbounds")
                    ?: result.data?.optJSONArray("data")
            }
            if (outbounds == null || outbounds.length() == 0) {
                toast("В профиле не найден поддерживаемый outbound")
                return@launch
            }
            val name = if (isWireGuard) {
                "WireGuard ${ProfileStore.items.size + 1}"
            } else {
                android.net.Uri.decode(value.substringAfter('#', ""))
                    .take(60)
                    .ifBlank { "Профиль ${ProfileStore.items.size + 1}" }
            }
            ProfileStore.add(name, outbounds.toString())
            toast("Профиль добавлен: $name")
        }
    }

    fun importSubscription(url: String) {
        if (url.isBlank()) {
            toast("Вставьте URL подписки")
            return
        }
        scope.launch {
            try {
                val result = SubscriptionManager.add(context, url.trim())
                if (result.error != null) {
                    toast("Подписка сохранена, но не обновилась: ${result.error}")
                } else {
                    toast("Из подписки добавлено профилей: ${result.added}")
                }
            } catch (error: Throwable) {
                toast(error.message ?: "Не удалось загрузить подписку")
            }
        }
    }

    fun refreshSubscription(id: Long) {
        scope.launch {
            val result = SubscriptionManager.refresh(context, id)
            if (result.error != null) {
                toast("Не удалось обновить: ${result.error}")
            } else {
                toast("Профилей в подписке: ${result.added}")
            }
        }
    }

    fun removeSubscription(id: Long) {
        scope.launch {
            SubscriptionManager.remove(context, id)
            toast("Подписка удалена")
        }
    }

    fun testProfiles() {
        if (tunnel.running) {
            toast("Сначала остановите VPN")
            return
        }
        val snapshot = ProfileStore.items.toList()
        if (snapshot.isEmpty()) {
            toast("Нет профилей для проверки")
            return
        }
        scope.launch {
            try {
                val configs = snapshot.map { profile ->
                    JSONObject()
                        .put("outbounds", JSONArray(profile.outboundsJson))
                        .toString()
                }
                val results = withContext(Dispatchers.IO) {
                    XrayBridge.pingBatch(configs)
                }
                var bestId: Long? = null
                var bestMs = Long.MAX_VALUE
                snapshot.forEachIndexed { index, profile ->
                    val result = results.getOrNull(index)
                    val ms = result?.takeIf { it.success }?.delayMs
                    ProfileStore.setLatency(profile.id, ms)
                    if (ms != null && ms < bestMs) {
                        bestMs = ms
                        bestId = profile.id
                    }
                }
                bestId?.let { ProfileStore.select(it) }
                val working = results.count { it.success }
                if (bestId == null) {
                    toast("Ни один профиль не ответил")
                } else {
                    toast("Лучший профиль выбран: $working из ${snapshot.size}, $bestMs мс")
                }
            } catch (error: Throwable) {
                toast(error.message ?: "Не удалось проверить профили")
            }
        }
    }

    fun toggleTunnel() {
        if (mode == Mode.Telegram) {
            if (telegram.running) {
                TelegramProxyService.stop(context)
                toast("Останавливаю Telegram-прокси…")
            } else {
                runCatching { TelegramProxyService.start(context, AppSettings.dpiEnabled) }
                    .onSuccess {
                        toast(
                            if (AppSettings.dpiEnabled) {
                                "Запускаю прокси с обходом DPI…"
                            } else {
                                "Запускаю Telegram-прокси…"
                            },
                        )
                    }
                    .onFailure { toast(it.message ?: "Не удалось запустить прокси") }
            }
            return
        }
        if (tunnel.running) {
            WireproxyVpnService.stop(context)
            return
        }
        val profile = ProfileStore.active
        if (profile == null) {
            toast("Сначала импортируйте VPN-профиль")
            tab = Tab.Profiles
            return
        }
        if (!vpnPermissionGranted) {
            onRequestVpnPermission()
            toast("Подтвердите разрешение VPN и нажмите ещё раз")
            return
        }
        runCatching {
            val outbounds = JSONArray(profile.outboundsJson)
            val config = VpnConfigBuilder.build(
                outbounds = outbounds,
                dpi = AppSettings.dpiEnabled,
                directDomains = AppSettings.directDomainList(),
            )
            WireproxyVpnService.start(context, config)
        }.onSuccess {
            toast("Подключаю ${profile.name}…")
        }.onFailure {
            toast(it.message ?: "Не удалось запустить VPN")
        }
    }

    fun openTelegram(link: String) {
        if (link.isBlank()) {
            toast("Сначала запустите Telegram-прокси")
            return
        }
        runCatching {
            context.startActivity(
                Intent(Intent.ACTION_VIEW, Uri.parse(link))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        }.onFailure {
            toast("Telegram не найден. Скопируйте ссылку и вставьте её в Telegram")
        }
    }

    fun copyTelegramLink(link: String) {
        if (link.isBlank()) {
            toast("Сначала запустите Telegram-прокси")
            return
        }
        clipboard.setText(AnnotatedString(link))
        toast("Ссылка скопирована")
    }

    LaunchedEffect(ImportBus.pending) {
        val link = ImportBus.pending ?: return@LaunchedEffect
        ImportBus.clear()
        tab = Tab.Profiles
        importProfile(link)
    }

    Scaffold(
        contentWindowInsets = WindowInsets.safeDrawing,
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text("Wireproxy", fontWeight = FontWeight.SemiBold)
                        Text(
                            "VPN · Telegram · DPI bypass",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
            )
        },
        bottomBar = {
            NavigationBar {
                Tab.entries.forEach { item ->
                    NavigationBarItem(
                        selected = tab == item,
                        onClick = { tab = item },
                        icon = { Icon(item.icon, contentDescription = item.label) },
                        label = { Text(item.label) },
                    )
                }
            }
        },
    ) { padding ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            when (tab) {
                Tab.Home -> HomeScreen(
                    mode = mode,
                    onMode = { mode = it },
                    dpiEnabled = AppSettings.dpiEnabled,
                    onDpi = AppSettings::updateDpi,
                    vpnPermissionGranted = vpnPermissionGranted,
                    tunnel = tunnel,
                    telegram = telegram,
                    activeRunning = activeRunning,
                    profileName = ProfileStore.active?.name,
                    onCheckXray = {
                        scope.launch { snackbar.showSnackbar("Xray: ${XrayBridge.version()}") }
                    },
                    onConnect = ::toggleTunnel,
                    onOpenTelegram = { openTelegram(telegram.link) },
                    onCopyTelegramLink = { copyTelegramLink(telegram.link) },
                )
                Tab.Profiles -> ProfilesScreen(
                    onImport = ::importProfile,
                    onSubscription = ::importSubscription,
                    onRefreshSubscription = ::refreshSubscription,
                    onRemoveSubscription = ::removeSubscription,
                    onTestAll = ::testProfiles,
                )
                Tab.Settings -> SettingsScreen(
                    dpiEnabled = AppSettings.dpiEnabled,
                    onDpi = AppSettings::updateDpi,
                )
            }
        }
    }
}

@Composable
private fun HomeScreen(
    mode: Mode,
    onMode: (Mode) -> Unit,
    dpiEnabled: Boolean,
    onDpi: (Boolean) -> Unit,
    vpnPermissionGranted: Boolean,
    tunnel: TunnelStatus,
    telegram: TelegramStatus,
    activeRunning: Boolean,
    profileName: String?,
    onCheckXray: () -> Unit,
    onConnect: () -> Unit,
    onOpenTelegram: () -> Unit,
    onCopyTelegramLink: () -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(28.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.45f),
        ),
    ) {
        Column(
            modifier = Modifier.padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Box(
                modifier = Modifier
                    .size(82.dp)
                    .background(
                        MaterialTheme.colorScheme.primary.copy(alpha = 0.12f),
                        RoundedCornerShape(50),
                    ),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    Icons.Default.Security,
                    contentDescription = null,
                    modifier = Modifier.size(40.dp),
                    tint = MaterialTheme.colorScheme.primary,
                )
            }
            Spacer(Modifier.height(16.dp))
            Text(
                when {
                    !activeRunning -> "Подключение выключено"
                    mode == Mode.Vpn -> "VPN включён"
                    else -> "Telegram-прокси включён"
                },
                style = MaterialTheme.typography.headlineSmall,
            )
            Text(
                when {
                    activeRunning && mode == Mode.Vpn -> tunnel.message
                    activeRunning -> telegram.message
                    mode == Mode.Vpn -> "Весь трафик телефона пойдёт через VPN"
                    else -> "Telegram пойдёт через локальный MTProto/WSS-прокси"
                },
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(20.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Mode.entries.forEach { item ->
                    FilterChip(
                        selected = mode == item,
                        onClick = { onMode(item) },
                        label = { Text(item.label) },
                        leadingIcon = {
                            Icon(
                                if (item == Mode.Vpn) Icons.Default.SwapHoriz else Icons.Default.Bolt,
                                contentDescription = null,
                                modifier = Modifier.size(16.dp),
                            )
                        },
                    )
                }
            }
            Spacer(Modifier.height(18.dp))
            Button(
                onClick = onConnect,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(60.dp),
                shape = RoundedCornerShape(18.dp),
            ) {
                Icon(Icons.Default.PowerSettingsNew, contentDescription = null)
                Spacer(Modifier.size(10.dp))
                Text(
                    if (activeRunning) "Отключить" else "Подключить",
                    style = MaterialTheme.typography.titleMedium,
                )
            }
            Spacer(Modifier.height(10.dp))
            Text(
                when {
                    activeRunning && mode == Mode.Telegram -> telegram.message
                    activeRunning -> profileName ?: tunnel.message
                    tunnel.message.isNotBlank() && tunnel.message != "VPN выключен" &&
                        mode == Mode.Vpn -> tunnel.message
                    telegram.message.isNotBlank() && telegram.message != "Telegram-прокси выключен" &&
                        mode == Mode.Telegram -> telegram.message
                    mode == Mode.Vpn && !vpnPermissionGranted ->
                        "Сначала Android попросит разрешение VPN"
                    mode == Mode.Telegram -> "Telegram подключится к локальному прокси"
                    profileName != null -> "Профиль: $profileName"
                    else -> "Профиль ещё не выбран"
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }

    Card(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Default.Bolt, contentDescription = null, tint = MaterialTheme.colorScheme.tertiary)
            Spacer(Modifier.size(12.dp))
            Column(Modifier.weight(1f)) {
                Text("Обход DPI", fontWeight = FontWeight.Medium)
                Text(
                    "TLS fragment в VPN · ByeDPI в Telegram",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Switch(checked = dpiEnabled, onCheckedChange = onDpi)
        }
    }

    if (mode == Mode.Telegram) {
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Локальный прокси", fontWeight = FontWeight.Medium)
                Text(
                    if (telegram.running) {
                        "127.0.0.1:1443 · клиентов: ${telegram.clients} · " +
                            "${formatBytes(telegram.bytesUp)} ↑ / ${formatBytes(telegram.bytesDown)} ↓"
                    } else {
                        "Запустите прокси и откройте ссылку в Telegram"
                    },
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = onOpenTelegram, enabled = telegram.running) {
                        Text("Открыть в Telegram")
                    }
                    Button(
                        onClick = onCopyTelegramLink,
                        enabled = telegram.running,
                        colors = androidx.compose.material3.ButtonDefaults.outlinedButtonColors(),
                    ) {
                        Text("Скопировать ссылку")
                    }
                }
                if (telegram.running) {
                    Text(
                        "Секрет: ${telegram.link.substringAfter("secret=", "").take(6)}… " +
                            "(хранится в Android Keystore)",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text("Что уже готово", fontWeight = FontWeight.Medium)
            Text(
                "• профили: VLESS/VMess/Trojan/SS/Outline, WireGuard, подписки\n" +
                    "• полный VPN через Xray и hev-socks5-tunnel\n" +
                    "• Telegram-only MTProto/WSS-прокси на 127.0.0.1\n" +
                    "• ByeDPI без root и автовыбор быстрейшего профиля",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Button(
                onClick = onCheckXray,
                colors = androidx.compose.material3.ButtonDefaults.outlinedButtonColors(),
            ) {
                Text("Проверить Xray-core")
            }
        }
    }
}

private fun formatBytes(value: Long): String {
    if (value < 1024) return "$value Б"
    var size = value.toDouble()
    for (unit in listOf("КБ", "МБ", "ГБ")) {
        size /= 1024
        if (size < 1024) return "%.1f %s".format(size, unit)
    }
    return "%.1f ТБ".format(size / 1024)
}

private fun formatTime(timestamp: Long): String =
    java.text.SimpleDateFormat("dd.MM HH:mm", java.util.Locale.getDefault())
        .format(java.util.Date(timestamp))

@Composable
private fun ProfilesScreen(
    onImport: (String) -> Unit,
    onSubscription: (String) -> Unit,
    onRefreshSubscription: (Long) -> Unit,
    onRemoveSubscription: (Long) -> Unit,
    onTestAll: () -> Unit,
) {
    var link by rememberSaveable { mutableStateOf("") }
    var subscription by rememberSaveable { mutableStateOf("") }
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("Профили", style = MaterialTheme.typography.titleLarge)
            Text(
                "Вставьте vless://, vmess://, trojan://, ss://, " +
                    "WireGuard [Interface]/[Peer] или ссылку подписки.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedTextField(
                value = link,
                onValueChange = { link = it },
                modifier = Modifier.fillMaxWidth(),
                label = { Text("Ссылка профиля") },
                placeholder = { Text("vless://…") },
                minLines = 2,
                maxLines = 4,
            )
            Button(
                onClick = {
                    onImport(link)
                    link = ""
                },
                enabled = link.isNotBlank(),
            ) {
                Text("Импортировать")
            }
            OutlinedTextField(
                value = subscription,
                onValueChange = { subscription = it },
                modifier = Modifier.fillMaxWidth(),
                label = { Text("URL подписки") },
                placeholder = { Text("https://…/subscription.txt") },
                singleLine = true,
            )
            Button(
                onClick = {
                    onSubscription(subscription)
                    subscription = ""
                },
                enabled = subscription.isNotBlank(),
            ) {
                Text("Добавить подписку")
            }
            Button(
                onClick = onTestAll,
                enabled = ProfileStore.items.isNotEmpty(),
            ) {
                Text("Проверить все и выбрать лучший")
            }
        }
    }

    if (SubscriptionStore.items.isNotEmpty()) {
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(Modifier.padding(12.dp)) {
                Text(
                    "Подписки",
                    fontWeight = FontWeight.Medium,
                    modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
                )
                SubscriptionStore.items.forEach { subscription ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(Modifier.weight(1f)) {
                            Text(subscription.name, fontWeight = FontWeight.Medium)
                            Text(
                                subscription.lastError
                                    ?: if (subscription.updatedAt > 0) {
                                        "Обновлено: ${formatTime(subscription.updatedAt)}"
                                    } else {
                                        "Ещё не обновлялась"
                                    },
                                style = MaterialTheme.typography.bodySmall,
                                color = if (subscription.lastError != null) {
                                    MaterialTheme.colorScheme.error
                                } else {
                                    MaterialTheme.colorScheme.onSurfaceVariant
                                },
                            )
                        }
                        androidx.compose.material3.TextButton(
                            onClick = { onRefreshSubscription(subscription.id) },
                        ) {
                            Text("Обновить")
                        }
                        IconButton(onClick = { onRemoveSubscription(subscription.id) }) {
                            Icon(Icons.Default.Delete, contentDescription = "Удалить подписку")
                        }
                    }
                }
            }
        }
    }

    if (ProfileStore.items.isEmpty()) {
        Text(
            "Профилей пока нет",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    } else {
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(Modifier.padding(12.dp)) {
                ProfileStore.items.forEach { profile ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(
                            selected = ProfileStore.activeId == profile.id,
                            onClick = { ProfileStore.select(profile.id) },
                        )
                        Column(Modifier.weight(1f)) {
                            Text(profile.name, fontWeight = FontWeight.Medium)
                            Text(
                                profile.latencyMs?.let { "$it мс" } ?: "Не проверен",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        IconButton(onClick = { ProfileStore.remove(profile.id) }) {
                            Icon(Icons.Default.Delete, contentDescription = "Удалить профиль")
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun SettingsScreen(dpiEnabled: Boolean, onDpi: (Boolean) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val update by UpdateManager.state.collectAsState()
    var directDomains by remember { mutableStateOf(AppSettings.directDomains) }
    var showApps by remember { mutableStateOf(false) }
    fun toast(text: String) {
        android.widget.Toast.makeText(context, text, android.widget.Toast.LENGTH_LONG).show()
    }
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("Обновления", style = MaterialTheme.typography.titleLarge)
            Text(
                "Установлено: ${UpdateManager.currentVersion}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            update.available?.let { available ->
                Text(
                    "Доступна версия ${available.version}",
                    fontWeight = FontWeight.Medium,
                )
                if (available.notes.isNotBlank()) {
                    Text(
                        available.notes.take(400),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (update.message.isNotBlank()) {
                Text(
                    update.message,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            update.error?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(
                    onClick = { scope.launch { UpdateManager.check(context) } },
                    enabled = !update.checking && !update.downloading,
                ) {
                    Text(if (update.checking) "Проверяю…" else "Проверить обновления")
                }
                if (update.available != null) {
                    Button(
                        onClick = {
                            scope.launch {
                                when (val result = UpdateManager.downloadAndInstall(context)) {
                                    InstallResult.Started -> toast("Открыт установщик Android")
                                    InstallResult.NeedsPermission ->
                                        toast("Разрешите установку из этого приложения и повторите")
                                    is InstallResult.Failed -> toast(result.message)
                                }
                            }
                        },
                        enabled = !update.checking && !update.downloading,
                    ) {
                        Text(
                            if (update.downloading) {
                                if (update.progress >= 0) "Скачивание ${update.progress}%" else "Скачивание…"
                            } else {
                                "Скачать и установить"
                            },
                        )
                    }
                }
            }
        }
    }
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Настройки", style = MaterialTheme.typography.titleLarge)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Обход DPI")
                    Text(
                        "TLS fragment в VPN; ByeDPI в Telegram-режиме",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Switch(checked = dpiEnabled, onCheckedChange = onDpi)
            }
            Text("Стратегия ByeDPI", fontWeight = FontWeight.Medium)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(
                    "light" to "Лёгкая",
                    "medium" to "Средняя",
                    "aggressive" to "Агрессивная",
                ).forEach { (id, label) ->
                    FilterChip(
                        selected = AppSettings.dpiMode == id,
                        onClick = { AppSettings.updateDpiMode(id) },
                        enabled = dpiEnabled,
                        label = { Text(label) },
                    )
                }
            }
            Text("Домены напрямую", fontWeight = FontWeight.Medium)
            OutlinedTextField(
                value = directDomains,
                onValueChange = {
                    directDomains = it
                    AppSettings.updateDirectDomains(it)
                },
                modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("example.com\nanother.org") },
                minLines = 3,
                maxLines = 6,
            )
            Text(
                "Сайты из списка пойдут мимо VPN-профиля. При включённом обходе DPI " +
                    "Xray фрагментирует их TLS-рукопожатие.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Button(
                onClick = { showApps = true },
                colors = androidx.compose.material3.ButtonDefaults.outlinedButtonColors(),
            ) {
                Text("Приложения вне VPN: ${AppSettings.excludedPackages.size}")
            }
            Button(
                onClick = {
                    runCatching {
                        context.startActivity(
                            Intent(Settings.ACTION_VPN_SETTINGS)
                                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                        )
                    }
                },
                colors = androidx.compose.material3.ButtonDefaults.outlinedButtonColors(),
            ) {
                Text("Always-on VPN и Kill Switch")
            }
            Text(
                "Включите Always-on VPN и «Блокировать соединения без VPN», чтобы " +
                    "после падения туннеля трафик не уходил напрямую.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text("Версия ${BuildConfig.VERSION_NAME}", style = MaterialTheme.typography.bodySmall)
            Text(
                "Desktop-версия: wireproxy-control",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }

    if (showApps) {
        val apps = remember { loadInstalledApps(context) }
        AlertDialog(
            onDismissRequest = { showApps = false },
            title = { Text("Приложения вне VPN") },
            text = {
                LazyColumn(Modifier.heightIn(max = 420.dp)) {
                    items(apps, key = { it.packageName }) { app ->
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = 2.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Checkbox(
                                checked = app.packageName in AppSettings.excludedPackages,
                                onCheckedChange = { checked ->
                                    val next = AppSettings.excludedPackages.toMutableSet()
                                    if (checked) next += app.packageName
                                    else next -= app.packageName
                                    AppSettings.updateExcludedPackages(next)
                                },
                            )
                            Column(Modifier.weight(1f)) {
                                Text(app.label)
                                Text(
                                    app.packageName,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { showApps = false }) { Text("Готово") }
            },
        )
    }
}

private data class InstalledApp(val label: String, val packageName: String)

private fun loadInstalledApps(context: android.content.Context): List<InstalledApp> {
    val manager = context.packageManager
    val intent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
    return manager.queryIntentActivities(intent, 0)
        .mapNotNull { info ->
            val packageName = info.activityInfo?.packageName ?: return@mapNotNull null
            if (packageName == context.packageName) return@mapNotNull null
            val label = runCatching { info.loadLabel(manager).toString() }
                .getOrDefault(packageName)
            InstalledApp(label, packageName)
        }
        .distinctBy { it.packageName }
        .sortedBy { it.label.lowercase() }
}
