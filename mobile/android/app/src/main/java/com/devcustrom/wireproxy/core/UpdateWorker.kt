package com.devcustrom.wireproxy.core

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.devcustrom.wireproxy.MainActivity
import java.util.concurrent.TimeUnit

class UpdateWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        UpdatePrefs.init(applicationContext)
        val update = UpdateManager.check(applicationContext) ?: return Result.success()
        if (UpdatePrefs.notifiedVersion() == update.version) return Result.success()
        notify(update)
        UpdatePrefs.setNotifiedVersion(applicationContext, update.version)
        return Result.success()
    }

    private fun notify(update: AppUpdate) {
        val manager = applicationContext.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL,
                "Обновления приложения",
                NotificationManager.IMPORTANCE_DEFAULT,
            ),
        )
        val intent = PendingIntent.getActivity(
            applicationContext,
            2,
            Intent(applicationContext, MainActivity::class.java)
                .putExtra("open_updates", true)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val notification: Notification = Notification.Builder(applicationContext, CHANNEL)
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setContentTitle("Доступно обновление ${update.version}")
            .setContentText("Нажмите, чтобы открыть Wireproxy и установить APK")
            .setContentIntent(intent)
            .setAutoCancel(true)
            .build()
        runCatching { manager.notify(NOTIFICATION_ID, notification) }
    }

    companion object {
        private const val CHANNEL = "app_updates"
        private const val NOTIFICATION_ID = 1081
    }
}

object UpdateScheduler {
    private const val PERIODIC = "wireproxy-app-update-check"
    private const val NOW = "wireproxy-app-update-now"

    fun schedule(context: Context) {
        UpdatePrefs.init(context)
        val request = PeriodicWorkRequestBuilder<UpdateWorker>(12, TimeUnit.HOURS)
            .setConstraints(
                Constraints.Builder()
                    .setRequiredNetworkType(NetworkType.CONNECTED)
                    .build(),
            )
            .build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(
            PERIODIC,
            ExistingPeriodicWorkPolicy.KEEP,
            request,
        )
        if (System.currentTimeMillis() - UpdatePrefs.lastCheck() > 12 * 60 * 60 * 1000L) {
            WorkManager.getInstance(context).enqueueUniqueWork(
                NOW,
                ExistingWorkPolicy.KEEP,
                OneTimeWorkRequestBuilder<UpdateWorker>()
                    .setConstraints(
                        Constraints.Builder()
                            .setRequiredNetworkType(NetworkType.CONNECTED)
                            .build(),
                    )
                    .build(),
            )
        }
    }

    fun checkNow(context: Context) {
        WorkManager.getInstance(context).enqueueUniqueWork(
            NOW,
            ExistingWorkPolicy.REPLACE,
            OneTimeWorkRequestBuilder<UpdateWorker>()
                .setConstraints(
                    Constraints.Builder()
                        .setRequiredNetworkType(NetworkType.CONNECTED)
                        .build(),
                )
                .build(),
        )
    }
}
