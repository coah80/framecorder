package com.framecorder.app.sync

import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import com.framecorder.app.FramecorderApp
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.sample
import kotlinx.coroutines.launch

/**
 * Keeps the process (and so the sync core) alive with the app closed. The
 * core does the work; this only shows what it's doing.
 */
class SyncService : LifecycleService() {
    @OptIn(FlowPreview::class)
    override fun onCreate() {
        super.onCreate()
        val app = application as FramecorderApp
        ServiceCompat.startForeground(
            this,
            Notifier.SYNCING_ID,
            app.notifier.syncing(app.hub.frames.value, app.hub.transfer.value, app.hub.remotes.value),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
        )
        lifecycleScope.launch {
            // a notification a second at most, however fast progress comes
            combine(app.hub.frames, app.hub.transfer, app.hub.remotes) { f, t, r -> Triple(f, t, r) }
                .sample(1000)
                .collect { (frames, transfer, remotes) -> app.notifier.updateSyncing(frames, transfer, remotes) }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)
        return START_STICKY
    }

    /** Android 15 caps data sync services at 6 h a day; after that, sync only runs with the app open. */
    override fun onTimeout(startId: Int, fgsType: Int) {
        stopSelf()
    }

    companion object {
        fun start(context: Context) {
            ContextCompat.startForegroundService(context, Intent(context, SyncService::class.java))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, SyncService::class.java))
        }
    }
}
