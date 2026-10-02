package com.framecorder.app

import android.app.Application
import com.framecorder.app.sync.Notifier
import com.framecorder.app.sync.SyncHub
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

class FramecorderApp : Application() {
    lateinit var hub: SyncHub
        private set
    lateinit var prefs: Prefs
        private set
    lateinit var notifier: Notifier
        private set

    /** Lives as long as the process. */
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    override fun onCreate() {
        super.onCreate()
        prefs = Prefs(this)
        notifier = Notifier(this)
        hub = SyncHub(this)
        hub.start()
        scope.launch {
            hub.synced.collect { clip -> notifier.clipArrived(clip, hub.frames.value.firstOrNull { it.fingerprint == clip.host }?.name, hub.gallery) }
        }
    }
}
