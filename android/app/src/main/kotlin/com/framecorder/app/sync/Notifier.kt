package com.framecorder.app.sync

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import com.framecorder.app.MainActivity
import com.framecorder.app.R
import com.framecorder.app.ui.common.length
import com.framecorder.app.ui.common.rate
import com.framecorder.app.ui.common.size
import com.framecorder.core.Clip
import com.framecorder.core.FrameState
import com.framecorder.core.FrameStatus

/** framecorder's notifications: the quiet one while syncing, and one per new clip. */
class Notifier(private val context: Context) {
    private val manager = context.getSystemService(NotificationManager::class.java)

    init {
        manager.createNotificationChannels(
            listOf(
                NotificationChannel(SYNC, context.getString(R.string.channel_sync), NotificationManager.IMPORTANCE_LOW)
                    .apply { description = context.getString(R.string.channel_sync_about); setShowBadge(false) },
                NotificationChannel(CLIPS, context.getString(R.string.channel_clips), NotificationManager.IMPORTANCE_DEFAULT)
                    .apply { description = context.getString(R.string.channel_clips_about) },
            ),
        )
    }

    private fun canPost() =
        ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    private fun openApp(clipKey: String? = null): PendingIntent {
        val intent = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
            .apply { if (clipKey != null) putExtra(MainActivity.EXTRA_CLIP, clipKey) }
        return PendingIntent.getActivity(context, clipKey.hashCode(), intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    /** The foreground service's notification: a recording, what's syncing, or that it's waiting. */
    fun syncing(frames: List<FrameStatus>, transfer: Transfer?, remotes: Map<String, RemoteNow> = emptyMap()): Notification {
        val builder = NotificationCompat.Builder(context, SYNC)
            .setSmallIcon(R.drawable.ic_stat_framecorder)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setContentIntent(openApp())
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
        val recording = frames.firstNotNullOfOrNull { f ->
            remotes[f.fingerprint]?.takeIf { f.state == FrameState.CONNECTED && it.state.recording }?.let { f to it }
        }
        if (recording != null) {
            val (frame, remote) = recording
            builder
                .setContentTitle("${frame.name} is recording")
                .setContentText(
                    if (remote.state.running) "stop it here or on the headset"
                    else "paused at ${length(remote.elapsedMs() / 1000.0)} while the framecorder tab is open",
                )
                .setCategory(NotificationCompat.CATEGORY_STATUS)
                .addAction(0, "stop recording", RemoteReceiver.intent(context, frame.fingerprint, "stop"))
            if (remote.state.running) {
                builder.setUsesChronometer(true).setShowWhen(true).setWhen(System.currentTimeMillis() - remote.elapsedMs())
            }
        } else if (transfer != null) {
            val frame = frames.firstOrNull { it.fingerprint == transfer.fingerprint }?.name ?: "your frame"
            val files = transfer.finished + 1 + transfer.queued
            val what = if (transfer.id.startsWith("r")) "a recording" else "a clip"
            val bytes = "${size(transfer.done)} of ${size(transfer.total)}" + if (transfer.bytesPerSecond > 0) " · ${rate(transfer.bytesPerSecond)}" else ""
            builder
                .setContentTitle(if (files > 1) "getting $files videos from $frame" else "getting $what from $frame")
                .setContentText(if (files > 1) "${transfer.finished + 1} of $files · $bytes" else bytes)
                .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            val done = (transfer.fraction * PER_FILE).toInt()
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA) {
                // a segment per file, filling up one after another
                val segments = if (files in 2..MAX_SEGMENTS) files else 1
                val progress = if (segments > 1) transfer.finished * PER_FILE + done else done
                builder.setStyle(
                    NotificationCompat.ProgressStyle()
                        .setProgressSegments(List(segments) { NotificationCompat.ProgressStyle.Segment(PER_FILE) })
                        .setProgress(progress)
                        .setStyledByProgress(true),
                )
            } else {
                builder.setProgress(PER_FILE, done, false)
            }
        } else {
            val connected = frames.filter { it.state == FrameState.CONNECTED }
            builder
                .setContentTitle(
                    when {
                        connected.isNotEmpty() -> "connected to ${connected.joinToString { it.name }}"
                        else -> "waiting for your frame"
                    },
                )
                .setContentText(
                    if (connected.isNotEmpty()) "new clips land here on their own"
                    else "syncs when it's on, on this wi-fi, with framecorder running",
                )
                .setCategory(NotificationCompat.CATEGORY_SERVICE)
            val clipping = connected.firstOrNull { remotes[it.fingerprint]?.state?.clipReady == true }
            if (clipping != null) builder.addAction(0, "save clip", RemoteReceiver.intent(context, clipping.fingerprint, "clip"))
        }
        return builder.build()
    }

    fun updateSyncing(frames: List<FrameStatus>, transfer: Transfer?, remotes: Map<String, RemoteNow>) {
        if (canPost()) manager.notify(SYNCING_ID, syncing(frames, transfer, remotes))
    }

    /** One per clip that lands, with its picture, share and open. */
    fun clipArrived(clip: Clip, from: String?, gallery: MediaGallery) {
        if (!canPost()) return
        val what = if (clip.kind == "clip") "clip" else "recording"
        val share = Intent.createChooser(
            Intent(Intent.ACTION_SEND)
                .setType("video/mp4")
                .putExtra(Intent.EXTRA_STREAM, Uri.parse(clip.location))
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION),
            "share $what",
        )
        val sharePending = PendingIntent.getActivity(context, clip.key.hashCode() + 1, share, PendingIntent.FLAG_IMMUTABLE)
        val builder = NotificationCompat.Builder(context, CLIPS)
            .setSmallIcon(R.drawable.ic_stat_framecorder)
            .setContentTitle("your $what from ${from ?: "your frame"} is here")
            .setContentText(listOfNotNull(clip.durationS?.let(::length), size(clip.size)).joinToString(" · "))
            .setContentIntent(openApp(clip.key))
            .setAutoCancel(true)
            .setGroup(CLIPS)
            .addAction(0, "share", sharePending)
            .addAction(0, "open", openApp(clip.key))
        gallery.thumbnail(clip.location)?.let { picture ->
            builder.setLargeIcon(picture).setStyle(NotificationCompat.BigPictureStyle().bigPicture(picture).bigLargeIcon(null as android.graphics.Bitmap?))
        }
        manager.notify(clip.key.hashCode(), builder.build())
    }

    companion object {
        const val SYNC = "sync"
        const val CLIPS = "clips"
        const val SYNCING_ID = 1
        private const val PER_FILE = 1000
        private const val MAX_SEGMENTS = 12
    }
}
