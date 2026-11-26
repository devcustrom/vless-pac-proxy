package com.devcustrom.wireproxy.core

import android.content.Context
import java.net.URLDecoder
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters

data class SubscriptionRefresh(
    val added: Int = 0,
    val error: String? = null,
)

object SubscriptionManager {
    private const val SYNC_NAME = "wireproxy-subscription-sync"
    private const val SYNC_NOW_NAME = "wireproxy-subscription-sync-now"
    private const val MAX_LINKS = 500
    private val supported = listOf("vless://", "vmess://", "trojan://", "ss://")

    suspend fun add(context: Context, url: String): SubscriptionRefresh {
        SubscriptionStore.init(context)
        val normalized = url.trim()
        require(normalized.startsWith("https://") || normalized.startsWith("http://")) {
            "Подписка должна быть http(s)-ссылкой"
        }
        val subscription = SubscriptionStore.add(normalized, nameFor(normalized))
        schedule(context)
        return refresh(context, subscription.id)
    }

    suspend fun refresh(context: Context, id: Long): SubscriptionRefresh {
        SubscriptionStore.init(context)
        val subscription = SubscriptionStore.items.firstOrNull { it.id == id }
            ?: return SubscriptionRefresh(error = "Подписка не найдена")
        return try {
            val text = SubscriptionFetcher.fetch(subscription.url)
            val links = SubscriptionFetcher.links(text)
                .filter { link -> supported.any { link.lowercase().startsWith(it) } }
                .distinct()
                .take(MAX_LINKS)
            if (links.isEmpty()) {
                SubscriptionStore.update(id, System.currentTimeMillis(), "В подписке нет ссылок")
                return SubscriptionRefresh(error = "В подписке не найдено поддерживаемых ссылок")
            }
            val parsed = withContext(Dispatchers.IO) {
                links.mapNotNull { link ->
                    val result = XrayBridge.convertShareLinks(link)
                    val outbounds = result.data?.optJSONArray("outbounds")
                        ?: result.data?.optJSONArray("data")
                    if (!result.success || outbounds == null || outbounds.length() == 0) {
                        null
                    } else {
                        profileName(link) to outbounds.toString()
                    }
                }
            }
            if (parsed.isEmpty()) {
                SubscriptionStore.update(id, System.currentTimeMillis(), "Не удалось разобрать профили")
                return SubscriptionRefresh(error = "Не удалось разобрать профили подписки")
            }
            ProfileStore.replaceSource(id, parsed)
            SubscriptionStore.update(id, System.currentTimeMillis(), null)
            SubscriptionRefresh(added = parsed.size)
        } catch (error: kotlinx.coroutines.CancellationException) {
            throw error
        } catch (error: Throwable) {
            val message = error.message ?: "Ошибка загрузки подписки"
            SubscriptionStore.update(id, System.currentTimeMillis(), message)
            SubscriptionRefresh(error = message)
        }
    }

    suspend fun remove(context: Context, id: Long) {
        SubscriptionStore.init(context)
        SubscriptionStore.remove(id)
        ProfileStore.removeSource(id)
        schedule(context)
    }

    suspend fun refreshAll(context: Context, force: Boolean = false): Int {
        SubscriptionStore.init(context)
        var updated = 0
        val now = System.currentTimeMillis()
        for (subscription in SubscriptionStore.items.toList()) {
            val stale = now - subscription.updatedAt > STALE_MS
            if (!force && !stale) continue
            val result = refresh(context, subscription.id)
            if (result.added > 0) updated += result.added
        }
        return updated
    }

    fun schedule(context: Context) {
        runCatching {
            SubscriptionStore.init(context)
            if (SubscriptionStore.items.isEmpty()) {
                WorkManager.getInstance(context).cancelUniqueWork(SYNC_NAME)
                return
            }
            val constraints = Constraints.Builder()
                .setRequiredNetworkType(NetworkType.CONNECTED)
                .build()
            val request = PeriodicWorkRequestBuilder<SubscriptionWorker>(12, TimeUnit.HOURS)
                .setConstraints(constraints)
                .build()
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(
                SYNC_NAME,
                ExistingPeriodicWorkPolicy.KEEP,
                request,
            )
        }
    }

    fun syncNow(context: Context) {
        runCatching {
            SubscriptionStore.init(context)
            if (SubscriptionStore.items.isEmpty()) return
            val request = androidx.work.OneTimeWorkRequestBuilder<SubscriptionWorker>()
                .setConstraints(
                    Constraints.Builder()
                        .setRequiredNetworkType(NetworkType.CONNECTED)
                        .build(),
                )
                .build()
            WorkManager.getInstance(context).enqueueUniqueWork(
                SYNC_NOW_NAME,
                androidx.work.ExistingWorkPolicy.KEEP,
                request,
            )
        }
    }

    private fun nameFor(url: String): String {
        val host = runCatching { java.net.URI(url).host }.getOrNull()
        return host?.takeIf { it.isNotBlank() } ?: "Подписка"
    }

    private fun profileName(link: String): String {
        val encoded = link.substringAfter('#', "")
        val decoded = runCatching {
            URLDecoder.decode(encoded, "UTF-8")
        }.getOrDefault(encoded)
        return decoded.take(60).ifBlank { "Профиль ${ProfileStore.items.size + 1}" }
    }

    private const val STALE_MS = 6 * 60 * 60 * 1000L
}

class SubscriptionWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        return runCatching {
            SubscriptionManager.refreshAll(applicationContext)
            Result.success()
        }.getOrElse {
            Result.retry()
        }
    }
}
