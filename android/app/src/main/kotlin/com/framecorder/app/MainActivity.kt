package com.framecorder.app

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.framecorder.app.sync.SyncService
import com.framecorder.app.ui.App
import com.framecorder.app.ui.theme.FramecorderTheme

class MainActivity : ComponentActivity() {
    /** A `framecorder://pair?...` link the camera app (or a notification) opened us with. */
    private var pendingLink by mutableStateOf<String?>(null)
    /** A clip a notification wants shown. */
    private var pendingClip by mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        take(intent)
        val app = application as FramecorderApp
        setContent {
            val wallpaper by app.prefs.wallpaper.collectAsStateWithLifecycle()
            FramecorderTheme(wallpaper = wallpaper) {
                App(
                    hub = app.hub,
                    prefs = app.prefs,
                    link = pendingLink,
                    clip = pendingClip,
                    onIntentHandled = {
                        pendingLink = null
                        pendingClip = null
                    },
                )
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        take(intent)
    }

    override fun onStart() {
        super.onStart()
        val app = application as FramecorderApp
        app.hub.retryNow()
        if (app.prefs.background.value && app.hub.frames.value.isNotEmpty()) SyncService.start(this)
    }

    private fun take(intent: Intent?) {
        val data = intent?.data
        if (data?.scheme == "framecorder" && data.host == "pair") pendingLink = data.toString()
        intent?.getStringExtra(EXTRA_CLIP)?.let { pendingClip = it }
    }

    companion object {
        const val EXTRA_CLIP = "clip"
    }
}
