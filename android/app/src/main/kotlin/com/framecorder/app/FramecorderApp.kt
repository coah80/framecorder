package com.framecorder.app

import android.app.Activity
import android.app.Application
import android.os.Bundle
import com.framecorder.app.sync.Notifier
import com.framecorder.app.sync.SyncHub
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged

class FramecorderApp : Application() {
    lateinit var hub: SyncHub
        private set
    lateinit var prefs: Prefs
        private set
    lateinit var notifier: Notifier
        private set
    lateinit var appUpdate: AppUpdate
        private set

    /** Lives as long as the process. */
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    override fun onCreate() {
        super.onCreate()
        prefs = Prefs(this)
        notifier = Notifier(this, prefs)
        hub = SyncHub(this)
        appUpdate = AppUpdate(prefs, scope)
        val started = MutableStateFlow(0)
        registerActivityLifecycleCallbacks(object : ActivityLifecycleCallbacks {
            override fun onActivityStarted(activity: Activity) { started.value++ }
            override fun onActivityStopped(activity: Activity) { started.value-- }
            override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) {}
            override fun onActivityResumed(activity: Activity) {}
            override fun onActivityPaused(activity: Activity) {}
            override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) {}
            override fun onActivityDestroyed(activity: Activity) {}
        })
        scope.launch {
            // Serialize stop/start so reopening can't race an unfinished stop.
            combine(prefs.background, started) { background, count -> background || count > 0 }
                .distinctUntilChanged()
                .collect { enabled -> if (enabled) hub.start() else hub.stop() }
        }
        scope.launch {
            hub.synced.collect { clip -> notifier.clipArrived(clip, hub.frames.value.firstOrNull { it.fingerprint == clip.host }?.name, hub.gallery) }
        }
    }
}
