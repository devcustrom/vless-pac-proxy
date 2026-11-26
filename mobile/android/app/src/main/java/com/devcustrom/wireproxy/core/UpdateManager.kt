package com.devcustrom.wireproxy.core

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import com.devcustrom.wireproxy.BuildConfig
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import org.json.JSONArray

data class AppUpdate(
    val version: String,
    val tag: String,
    val apkUrl: String,
    val sha256: String,
    val notes: String,
    val publishedAt: String,
)

data class UpdateState(
    val checking: Boolean = false,
    val available: AppUpdate? = null,
    val downloading: Boolean = false,
    val progress: Int = -1,
    val checkedAt: Long = 0,
    val error: String? = null,
    val message: String = "",
)

sealed interface InstallResult {
    data object Started : InstallResult
    data object NeedsPermission : InstallResult
    data class Failed(val message: String) : InstallResult
}

/**
 * Checks GitHub Releases and installs the signed Android APK.
 *
 * Updates are only offered for releases that contain
 * `wireproxy-control-android.apk` and a SHA-256 digest. The APK must be signed
 * with the same key as the installed build, otherwise Android rejects it.
 */
object UpdateManager {
    private const val API =
        "https://api.github.com/repos/devcustrom/vless-pac-proxy/releases?per_page=100"
    private const val ASSET_NAME = "wireproxy-control-android.apk"
    private const val MAX_DOWNLOAD = 200L * 1024 * 1024

    private val client = OkHttpClient.Builder()
        .connectTimeout(20, TimeUnit.SECONDS)
        .readTimeout(60, TimeUnit.SECONDS)
        .build()

    private val mutable = MutableStateFlow(UpdateState())
    val state: StateFlow<UpdateState> = mutable.asStateFlow()

    val currentVersion: String
        get() = BuildConfig.VERSION_NAME

    suspend fun check(context: Context): AppUpdate? = withContext(Dispatchers.IO) {
        mutable.value = mutable.value.copy(
            checking = true,
            error = null,
            message = "Проверяю GitHub Releases…",
        )
        try {
            val update = findUpdate(currentVersion)
            mutable.value = UpdateState(
                checking = false,
                available = update,
                checkedAt = System.currentTimeMillis(),
                message = if (update == null) {
                    "Установлена последняя версия"
                } else {
                    "Доступна версия ${update.version}"
                },
            )
            update
        } catch (error: CancellationException) {
            mutable.value = mutable.value.copy(checking = false, message = "")
            throw error
        } catch (error: Throwable) {
            mutable.value = mutable.value.copy(
                checking = false,
                error = error.message ?: "Не удалось проверить обновления",
                message = "",
            )
            null
        } finally {
            // Throttle retries after failures too: notifications and the
            // periodic worker must not hammer the GitHub API while offline.
            UpdatePrefs.setLastCheck(context)
        }
    }

    suspend fun downloadAndInstall(context: Context): InstallResult = withContext(Dispatchers.IO) {
        val update = mutable.value.available
            ?: return@withContext InstallResult.Failed("Нет доступного обновления")
        mutable.value = mutable.value.copy(
            downloading = true,
            progress = 0,
            error = null,
            message = "Скачиваю ${update.version}…",
        )
        try {
            val file = download(context, update)
            mutable.value = mutable.value.copy(
                downloading = false,
                progress = 100,
                message = "Запускаю установщик…",
            )
            install(context, file)
        } catch (error: CancellationException) {
            // Leaving the settings screen cancels the UI scope; reset the
            // card instead of leaving it stuck at "downloading" forever.
            mutable.value = mutable.value.copy(downloading = false, progress = -1, message = "")
            throw error
        } catch (error: Throwable) {
            mutable.value = mutable.value.copy(
                downloading = false,
                progress = -1,
                error = error.message ?: "Ошибка загрузки",
                message = "",
            )
            InstallResult.Failed(error.message ?: "Ошибка загрузки")
        }
    }

    fun install(context: Context, file: File): InstallResult {
        if (!file.isFile) return InstallResult.Failed("APK не найден")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
            !context.packageManager.canRequestPackageInstalls()
        ) {
            openUnknownSourcesSettings(context)
            return InstallResult.NeedsPermission
        }
        return runCatching {
            val uri = FileProvider.getUriForFile(
                context,
                "${context.packageName}.updates",
                file,
            )
            val intent = Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, "application/vnd.android.package-archive")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            intent.clipData = ClipData.newRawUri("", uri)
            context.startActivity(intent)
            InstallResult.Started
        }.getOrElse {
            InstallResult.Failed(it.message ?: "Не удалось открыть установщик")
        }
    }

    fun openUnknownSourcesSettings(context: Context) {
        runCatching {
            context.startActivity(
                Intent(
                    Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                    Uri.parse("package:${context.packageName}"),
                ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        }
    }

    private fun findUpdate(current: String): AppUpdate? {
        val request = Request.Builder()
            .url(API)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "wireproxy-control-android/${BuildConfig.VERSION_NAME}")
            .build()
        var best: AppUpdate? = null
        var problem: String? = null
        client.newCall(request).execute().use { response ->
            if (!response.isSuccessful) {
                error("GitHub API: HTTP ${response.code}")
            }
            val releases = JSONArray(response.body?.string().orEmpty())
            for (index in 0 until releases.length()) {
                val release = releases.optJSONObject(index) ?: continue
                if (release.optBoolean("draft") || release.optBoolean("prerelease")) continue
                val tag = release.optString("tag_name").removePrefix("v").trim()
                if (!isReleaseTag(tag) || !isNewer(tag, current)) continue
                // GitHub lists releases by creation date, which is not
                // necessarily semver order; keep the highest version.
                val known = best
                if (known != null && !isNewer(tag, known.version)) continue
                val assets = release.optJSONArray("assets") ?: JSONArray()
                val apk = findAsset(assets) ?: continue
                val apkUrl = apk.optString("browser_download_url")
                if (apkUrl.isBlank()) continue
                val sha = assetSha(assets, apk)
                if (sha == null) {
                    problem = "В релизе $tag нет SHA-256 для APK"
                    continue
                }
                best = AppUpdate(
                    version = tag,
                    tag = release.optString("tag_name"),
                    apkUrl = apkUrl,
                    sha256 = sha.lowercase(),
                    notes = release.optString("body").take(4000),
                    publishedAt = release.optString("published_at"),
                )
                problem = null
            }
        }
        return best ?: problem?.let { error(it) }
    }

    private fun isReleaseTag(tag: String): Boolean =
        Regex("^\\d+(\\.\\d+){0,3}$").matches(tag)

    private fun findAsset(assets: JSONArray): org.json.JSONObject? {
        var fallback: org.json.JSONObject? = null
        for (index in 0 until assets.length()) {
            val asset = assets.optJSONObject(index) ?: continue
            val name = asset.optString("name")
            if (name.equals(ASSET_NAME, ignoreCase = true)) return asset
            if (fallback == null && name.endsWith(".apk") && name.contains("android")) {
                fallback = asset
            }
        }
        return fallback
    }

    private fun assetSha(
        assets: JSONArray,
        apk: org.json.JSONObject,
    ): String? {
        val digest = apk.optString("digest")
        if (digest.startsWith("sha256:")) {
            return digest.removePrefix("sha256:")
        }
        val shaAsset = (0 until assets.length())
            .mapNotNull { assets.optJSONObject(it) }
            .firstOrNull { it.optString("name") == apk.optString("name") + ".sha256" }
            ?: return null
        val url = shaAsset.optString("browser_download_url")
        if (url.isBlank()) return null
        val request = Request.Builder()
            .url(url)
            .header("User-Agent", "wireproxy-control-android/${BuildConfig.VERSION_NAME}")
            .build()
        client.newCall(request).execute().use { response ->
            if (!response.isSuccessful) return null
            val text = response.body?.string().orEmpty()
            return Regex("[0-9a-fA-F]{64}").find(text)?.value
        }
    }

    private fun download(context: Context, update: AppUpdate): File {
        val dir = File(context.cacheDir, "updates").apply { mkdirs() }
        dir.listFiles()?.forEach { old ->
            if (old.name != "update-${update.version}.apk") old.delete()
        }
        val target = File(dir, "update-${update.version}.apk")
        if (target.isFile && sha256(target).equals(update.sha256, ignoreCase = true)) {
            return target
        }
        target.delete()
        val part = File(dir, target.name + ".part")
        part.delete()
        val request = Request.Builder()
            .url(update.apkUrl)
            .header("User-Agent", "wireproxy-control-android/${BuildConfig.VERSION_NAME}")
            .build()
        client.newCall(request).execute().use { response ->
            if (!response.isSuccessful) error("APK: HTTP ${response.code}")
            val body = response.body ?: error("Пустой ответ APK")
            val length = body.contentLength()
            if (length > MAX_DOWNLOAD) error("APK слишком большой")
            val digest = MessageDigest.getInstance("SHA-256")
            var total = 0L
            body.byteStream().use { input ->
                part.outputStream().use { output ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        val read = input.read(buffer)
                        if (read < 0) break
                        total += read
                        if (total > MAX_DOWNLOAD) error("APK слишком большой")
                        output.write(buffer, 0, read)
                        digest.update(buffer, 0, read)
                        if (length > 0) {
                            val percent = ((total * 100) / length).toInt().coerceIn(0, 100)
                            mutable.value = mutable.value.copy(progress = percent)
                        }
                    }
                }
            }
            val actual = digest.digest().joinToString("") { "%02x".format(it.toInt() and 0xFF) }
            if (!actual.equals(update.sha256, ignoreCase = true)) {
                part.delete()
                error("SHA-256 не совпадает: скачанный файл повреждён")
            }
        }
        if (!part.renameTo(target)) {
            part.delete()
            error("Не удалось сохранить APK")
        }
        verifyPackage(context, target, update.version)
        return target
    }

    private fun verifyPackage(context: Context, file: File, expected: String) {
        val info = context.packageManager.getPackageArchiveInfo(file.absolutePath, 0)
            ?: error("Скачанный файл не является APK")
        if (info.packageName != context.packageName) {
            file.delete()
            error("APK другого приложения")
        }
        @Suppress("DEPRECATION")
        val version = info.versionName
        if (version.isNullOrBlank() || !version.removePrefix("v").equals(expected, ignoreCase = true)) {
            file.delete()
            error("В APK неверная версия ($version, ожидалась $expected)")
        }
    }

    private fun sha256(file: File): String {
        val digest = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val buffer = ByteArray(64 * 1024)
            while (true) {
                val read = input.read(buffer)
                if (read < 0) break
                digest.update(buffer, 0, read)
            }
        }
        return digest.digest().joinToString("") { "%02x".format(it.toInt() and 0xFF) }
    }

    fun isNewer(remote: String, current: String): Boolean {
        val left = versionParts(remote)
        val right = versionParts(current)
        if (left.isEmpty() || right.isEmpty()) return false
        for (index in 0 until maxOf(left.size, right.size)) {
            val a = left.getOrElse(index) { 0 }
            val b = right.getOrElse(index) { 0 }
            if (a != b) return a > b
        }
        return false
    }

    private fun versionParts(value: String): List<Int> =
        Regex("\\d+").findAll(value)
            .map { it.value.toIntOrNull() ?: 0 }
            .take(4)
            .toList()
}
