package com.framecorder.app.sync

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.graphics.drawable.Icon
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.widget.Toast
import com.framecorder.app.FramecorderApp
import com.framecorder.app.R
import com.framecorder.core.CoreException
import com.framecorder.core.FrameState
import com.framecorder.core.FrameStatus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** The frame a tile or a notification acts on: the first connected one whose tab is running. */
fun SyncHub.remoteFrame(): Pair<FrameStatus, RemoteNow>? =
    frames.value.firstNotNullOfOrNull { f ->
        remotes.value[f.fingerprint]?.takeIf { f.state == FrameState.CONNECTED && it.state.available && it.state.ready }?.let { f to it }
    }

/** Sends a command from outside the app, saying how it went in a toast. */
private fun FramecorderApp.send(fingerprint: String, what: String, after: () -> Unit = {}) {
    scope.launch {
        val said = try {
            hub.command(fingerprint, what)
            if (what == "clip") "saved, it'll be on your phone in a moment" else null
        } catch (e: CoreException) {
            e.reason()
        }
        withContext(Dispatchers.Main) {
            if (said != null) Toast.makeText(this@send, said, Toast.LENGTH_SHORT).show()
            after()
        }
    }
}

/** The buttons on the syncing notification: stop recording, save clip. */
class RemoteReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val fingerprint = intent.getStringExtra(EXTRA_FRAME) ?: return
        val what = intent.getStringExtra(EXTRA_DO) ?: return
        val pending = goAsync()
        (context.applicationContext as FramecorderApp).send(fingerprint, what) { pending.finish() }
    }

    companion object {
        private const val EXTRA_FRAME = "frame"
        private const val EXTRA_DO = "do"

        fun intent(context: Context, fingerprint: String, what: String): PendingIntent {
            val intent = Intent(context, RemoteReceiver::class.java).putExtra(EXTRA_FRAME, fingerprint).putExtra(EXTRA_DO, what)
            return PendingIntent.getBroadcast(context, (fingerprint + what).hashCode(), intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }
    }
}

/** A quick settings tile that follows the connected frame while it's on screen. */
abstract class RemoteTile : TileService() {
    private var watching: Job? = null
    protected val app get() = application as FramecorderApp

    override fun onStartListening() {
        super.onStartListening()
        watching = app.scope.launch(Dispatchers.Main) {
            combine(app.hub.frames, app.hub.remotes) { _, _ -> app.hub.remoteFrame() }.collect { show(it) }
        }
    }

    override fun onStopListening() {
        watching?.cancel()
        super.onStopListening()
    }

    private fun show(target: Pair<FrameStatus, RemoteNow>?) {
        val tile = qsTile ?: return
        draw(tile, target)
        tile.updateTile()
    }

    protected abstract fun draw(tile: Tile, target: Pair<FrameStatus, RemoteNow>?)

    protected fun send(what: String) {
        val (frame, _) = app.hub.remoteFrame() ?: return
        app.send(frame.fingerprint, what)
    }
}

class ClipTile : RemoteTile() {
    override fun draw(tile: Tile, target: Pair<FrameStatus, RemoteNow>?) {
        val ready = target != null && target.second.state.clipReady && !target.second.state.recording
        tile.icon = Icon.createWithResource(this, R.drawable.ic_tile_clip)
        tile.state = if (ready) Tile.STATE_ACTIVE else Tile.STATE_UNAVAILABLE
        tile.subtitle = when {
            target == null -> "no frame"
            target.second.state.recording -> "recording"
            target.second.state.clipSecs == null -> "clips off"
            else -> target.first.name
        }
    }

    override fun onClick() {
        super.onClick()
        if (qsTile?.state == Tile.STATE_ACTIVE) send("clip")
    }
}

class RecordTile : RemoteTile() {
    override fun draw(tile: Tile, target: Pair<FrameStatus, RemoteNow>?) {
        val recording = target?.second?.state?.recording == true
        tile.icon = Icon.createWithResource(this, R.drawable.ic_tile_record)
        tile.state = when {
            target == null -> Tile.STATE_UNAVAILABLE
            recording -> Tile.STATE_ACTIVE
            else -> Tile.STATE_INACTIVE
        }
        tile.subtitle = when {
            target == null -> "no frame"
            recording -> "recording"
            else -> "not recording"
        }
    }

    override fun onClick() {
        super.onClick()
        when (qsTile?.state) {
            Tile.STATE_ACTIVE -> send("stop")
            Tile.STATE_INACTIVE -> send("record")
        }
    }
}
