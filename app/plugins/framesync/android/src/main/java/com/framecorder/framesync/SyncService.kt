package com.framecorder.framesync

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import androidx.core.app.NotificationCompat

// Keeps the app's process in the foreground while it syncs, so Android
// doesn't freeze it the moment you switch apps. The syncing itself happens
// in Rust, this just holds the door open.
class SyncService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val manager = getSystemService(NotificationManager::class.java)
        val channel = NotificationChannel(CHANNEL, "Syncing", NotificationManager.IMPORTANCE_LOW)
        channel.description = "Shown while framecorder keeps clips in sync"
        channel.setShowBadge(false)
        manager.createNotificationChannel(channel)

        val launch = packageManager.getLaunchIntentForPackage(packageName)
        val open = PendingIntent.getActivity(this, 0, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val notification: Notification = NotificationCompat.Builder(this, CHANNEL)
            .setContentTitle("framecorder is syncing")
            .setContentText("new clips from your Frame land in Movies/framecorder while it's on and nearby")
            .setSmallIcon(android.R.drawable.stat_sys_download_done)
            .setOngoing(true)
            .setSilent(true)
            .setContentIntent(open)
            .build()
        startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        // if Android kills us anyway, don't come back as an empty shell: the
        // sync lives in the app, and it catches up next time it's opened
        return START_NOT_STICKY
    }

    // swiped out of recents: the app is closing, don't hang on to the notification
    override fun onTaskRemoved(rootIntent: Intent?) {
        stopSelf()
    }

    // Android 15 caps dataSync services at 6 hours a day
    override fun onTimeout(startId: Int, fgsType: Int) {
        stopSelf()
    }

    companion object {
        const val CHANNEL = "framecorder-sync"
        const val NOTIFICATION_ID = 7201
    }
}
