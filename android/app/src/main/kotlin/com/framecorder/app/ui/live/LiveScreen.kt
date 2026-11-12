@file:OptIn(UnstableApi::class)

package com.framecorder.app.ui.live

import android.content.ClipData
import android.content.ClipboardManager
import android.content.pm.ActivityInfo
import android.os.Build
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.annotation.OptIn
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.compose.ContentFrame
import androidx.media3.ui.compose.SURFACE_TYPE_TEXTURE_VIEW
import com.framecorder.app.FramecorderApp
import com.framecorder.app.sync.SyncHub
import com.framecorder.app.sync.reason
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.GroupLabel
import com.framecorder.app.ui.common.SegmentRow
import com.framecorder.app.ui.common.ShapeIcon
import com.framecorder.app.ui.common.StatusDot
import com.framecorder.app.ui.common.dotted
import com.framecorder.app.ui.common.rate
import com.framecorder.app.ui.common.stateColor
import com.framecorder.app.ui.common.stateText
import com.framecorder.app.ui.theme.DataStyle
import com.framecorder.app.ui.theme.LocalStatusColors
import com.framecorder.core.CoreException
import com.framecorder.core.FrameState
import com.framecorder.core.LivePhase
import com.framecorder.core.LiveStatus
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** How often the screen asks the core where live view stands while it's on. */
private const val LOOK_EVERY_MS = 500L

/** How long after the player loses the stream before it opens the URL again. */
private const val REOPEN_AFTER_MS = 1000L

/** How long the exit button stays up in fullscreen after a tap. */
private const val CONTROLS_FOR_MS = 3000L

/**
 * Live view: what the headset shows, played here a moment behind. It runs
 * only while this screen is up: leaving it, or the app, stops it, so the
 * headset never captures for nobody.
 */
@Composable
fun LiveScreen(hub: SyncHub, onFrame: (String) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    val frames by hub.frames.collectAsStateWithLifecycle()
    val remotes by hub.remotes.collectAsStateWithLifecycle()
    var picked by rememberSaveable { mutableStateOf<String?>(null) }
    val frame = frames.firstOrNull { it.fingerprint == picked }
        ?: frames.firstOrNull { it.state == FrameState.CONNECTED }
        ?: frames.firstOrNull()
    val connected = frame?.state == FrameState.CONNECTED
    val remote = frame?.let { remotes[it.fingerprint] }?.takeIf { connected }?.state
    // a headset too old for live view doesn't say how many are watching
    val canLive = remote != null && remote.liveViewers != null

    var wanted by remember { mutableStateOf(false) }
    var status by remember { mutableStateOf<LiveStatus?>(null) }
    var starting by remember { mutableStateOf(false) }

    val player = remember {
        ExoPlayer.Builder(context)
            // a live stream wants to start quickly, not fill a long buffer first
            .setLoadControl(DefaultLoadControl.Builder().setBufferDurationsMs(1500, 10_000, 500, 1000).build())
            .setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MOVIE).build(), true)
            .build()
    }
    val app = context.applicationContext as FramecorderApp
    DisposableEffect(Unit) {
        onDispose {
            player.release()
            app.scope.launch { hub.stopLive() }
        }
    }

    fun start() {
        val fp = frame?.fingerprint ?: return
        starting = true
        haptics.performHapticFeedback(HapticFeedbackType.Confirm)
        scope.launch {
            try {
                status = hub.startLive(fp)
                wanted = true
            } catch (e: CoreException) {
                haptics.performHapticFeedback(HapticFeedbackType.Reject)
                Toast.makeText(context, e.reason(), Toast.LENGTH_LONG).show()
            } finally {
                starting = false
            }
        }
    }

    fun stop() {
        wanted = false
        player.stop()
        player.clearMediaItems()
        scope.launch {
            hub.stopLive()
            status = hub.liveStatus()
        }
    }

    // while it's on: follow the core's state, and keep the player on the URL
    var lastOpen by remember { mutableLongStateOf(0L) }
    LaunchedEffect(wanted) {
        while (wanted) {
            val now = hub.liveStatus()
            status = now
            if (now?.phase == LivePhase.LIVE) {
                val idle = player.playbackState == Player.STATE_IDLE || player.playbackState == Player.STATE_ENDED
                val at = System.currentTimeMillis()
                if (idle && at - lastOpen >= REOPEN_AFTER_MS) {
                    lastOpen = at
                    player.setMediaItem(MediaItem.fromUri(now.url))
                    player.prepare()
                    player.playWhenReady = true
                }
            } else if (player.mediaItemCount > 0) {
                player.stop()
                player.clearMediaItems()
            }
            delay(LOOK_EVERY_MS)
        }
    }

    val scheme = MaterialTheme.colorScheme
    val statusColors = LocalStatusColors.current
    val phase = status?.phase?.takeIf { wanted }
    val live = phase == LivePhase.LIVE
    // fullscreen lasts as long as live view does, through reconnects
    var full by rememberSaveable { mutableStateOf(false) }
    if (!wanted) full = false
    // the screen stays on while it plays, or a cast goes dark
    val view = LocalView.current
    DisposableEffect(live) {
        view.keepScreenOn = live
        onDispose { view.keepScreenOn = false }
    }
    if (full) {
        FullscreenLive(player, live, phase, status?.error, onExit = { full = false })
    }
    Box(Modifier.fillMaxSize().background(scheme.surface)) {
        Column(
            Modifier.fillMaxSize().statusBarsPadding().verticalScroll(rememberScrollState()).padding(start = 16.dp, end = 16.dp, bottom = 140.dp),
        ) {
            Column(Modifier.padding(start = 4.dp, top = 56.dp, bottom = 18.dp)) {
                Text(dotted("live"), style = MaterialTheme.typography.displaySmall)
                Row(Modifier.padding(top = 2.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    if (frame != null) {
                        StatusDot(stateColor(frame))
                        Text(
                            "${frame.name} · ${stateText(frame)}",
                            style = MaterialTheme.typography.bodyMedium,
                            color = scheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
            if (frames.size > 1) {
                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(bottom = 12.dp)) {
                    items(frames, key = { it.fingerprint }) { f ->
                        FilterChip(
                            selected = f.fingerprint == frame?.fingerprint,
                            onClick = {
                                if (wanted) stop()
                                picked = f.fingerprint
                            },
                            label = { Text(f.name) },
                            leadingIcon = { StatusDot(stateColor(f)) },
                        )
                    }
                }
            }

            Box(
                Modifier
                    .fillMaxWidth()
                    .aspectRatio(16f / 9f)
                    .clip(RoundedCornerShape(24.dp))
                    .background(scheme.surfaceContainerHighest),
                contentAlignment = Alignment.Center,
            ) {
                if (live && !full) {
                    ContentFrame(player = player, surfaceType = SURFACE_TYPE_TEXTURE_VIEW, modifier = Modifier.fillMaxSize())
                    IconButton(
                        onClick = { full = true },
                        modifier = Modifier.align(Alignment.BottomEnd).padding(8.dp).clip(CircleShape).background(Color.Black.copy(alpha = 0.45f)),
                    ) {
                        Icon(FcIcons.Fullscreen, contentDescription = "fullscreen", tint = Color.White)
                    }
                } else if (!live) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        if (phase == LivePhase.CONNECTING) {
                            LoadingIndicator(Modifier.size(40.dp))
                        } else {
                            ShapeIcon(FcIcons.Live, shape = MaterialShapes.Sunny, size = 56.dp, iconSize = 26.dp, container = scheme.surfaceContainerHigh, content = scheme.onSurfaceVariant)
                        }
                        val said = when (phase) {
                            LivePhase.CONNECTING -> "connecting to ${frame?.name ?: "your frame"}"
                            LivePhase.ERROR -> status?.error ?: "live view stopped"
                            else -> null
                        }
                        if (said != null) Text(
                            said,
                            style = MaterialTheme.typography.bodyMedium,
                            color = scheme.onSurfaceVariant,
                            modifier = Modifier.padding(horizontal = 24.dp),
                        )
                    }
                }
            }

            Row(
                Modifier.fillMaxWidth().padding(top = 14.dp, start = 4.dp, end = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                StatusDot(
                    when (phase) {
                        LivePhase.LIVE -> statusColors.rec
                        LivePhase.CONNECTING -> scheme.primary
                        LivePhase.ERROR -> scheme.error
                        else -> scheme.outline
                    },
                )
                Text(
                    when (phase) {
                        LivePhase.LIVE -> "live"
                        LivePhase.CONNECTING -> "connecting"
                        LivePhase.ERROR -> "stopped"
                        else -> "off"
                    },
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.weight(1f),
                )
                status?.takeIf { live }?.let { s ->
                    Text(
                        listOfNotNull(
                            rate(s.bytesPerSecond.toDouble()).takeIf { s.bytesPerSecond > 0u },
                            "${s.viewers} watching here",
                        ).joinToString(" · "),
                        style = DataStyle,
                        color = scheme.onSurfaceVariant,
                    )
                }
            }

            Button(
                onClick = { if (wanted) stop() else start() },
                enabled = canLive && !starting,
                shapes = ButtonDefaults.shapes(),
                colors = if (wanted) {
                    ButtonDefaults.buttonColors(containerColor = statusColors.recContainer, contentColor = statusColors.onRecContainer)
                } else {
                    ButtonDefaults.buttonColors()
                },
                modifier = Modifier.fillMaxWidth().padding(top = 14.dp).height(56.dp),
            ) {
                Text(if (wanted) "stop" else "start live view")
            }
            val note = when {
                frame == null -> "pair a frame first."
                !connected -> "connect to ${frame.name} first. it has to be on, on this wi-fi, with framecorder running."
                remote == null -> "live view works once framecorder is open on the headset. it starts with SteamVR."
                !canLive -> "live view needs a newer framecorder on the headset."
                else -> null
            }
            if (note != null) Text(
                note,
                style = MaterialTheme.typography.bodySmall,
                color = scheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 10.dp),
            )

            val url = status?.url?.takeIf { wanted }
            if (canLive && url != null) {
                GroupLabel("in another app")
                SegmentRow(
                    0, 1,
                    url,
                    subtitle = "for VLC or another player",
                    leading = { Icon(FcIcons.Link, contentDescription = null, tint = scheme.onSurfaceVariant) },
                    trailing = {
                        IconButton(onClick = { copy(context, url) }) {
                            Icon(FcIcons.Copy, contentDescription = "copy the address", tint = scheme.primary)
                        }
                    },
                )
            } else if (!canLive && connected) {
                Spacer(Modifier.height(14.dp))
                Button(
                    onClick = { onFrame(frame.fingerprint) },
                    colors = ButtonDefaults.filledTonalButtonColors(),
                    modifier = Modifier.fillMaxWidth().height(48.dp),
                ) { Text("open the frame tab") }
            }
        }
    }
}

/**
 * The live picture on its own: black around it, no system bars, turned to fit
 * it, so the phone's screen can be cast or mirrored to a TV. Back, or the
 * button a tap brings up, leaves it.
 */
@Composable
private fun FullscreenLive(player: ExoPlayer, live: Boolean, phase: LivePhase?, error: String?, onExit: () -> Unit) {
    val activity = LocalActivity.current
    DisposableEffect(Unit) {
        val before = activity?.requestedOrientation
        val size = player.videoSize
        activity?.requestedOrientation = if (size.height > size.width) {
            ActivityInfo.SCREEN_ORIENTATION_SENSOR_PORTRAIT
        } else {
            ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        }
        onDispose { if (before != null) activity.requestedOrientation = before }
    }
    Dialog(
        onDismissRequest = onExit,
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        val view = LocalView.current
        DisposableEffect(view) {
            val window = (view.parent as? DialogWindowProvider)?.window
            val bars = window?.let { WindowCompat.getInsetsController(it, view) }
            bars?.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            bars?.hide(WindowInsetsCompat.Type.systemBars())
            view.keepScreenOn = true
            onDispose { view.keepScreenOn = false }
        }
        BackHandler(onBack = onExit)
        var tapped by remember { mutableLongStateOf(System.currentTimeMillis()) }
        var controls by remember { mutableStateOf(true) }
        LaunchedEffect(tapped) {
            controls = true
            delay(CONTROLS_FOR_MS)
            controls = false
        }
        Box(
            Modifier
                .fillMaxSize()
                .background(Color.Black)
                .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { tapped = System.currentTimeMillis() },
            contentAlignment = Alignment.Center,
        ) {
            if (live) {
                ContentFrame(player = player, surfaceType = SURFACE_TYPE_TEXTURE_VIEW, modifier = Modifier.fillMaxSize())
            } else if (phase == LivePhase.ERROR) {
                Text(error ?: "live view stopped", style = MaterialTheme.typography.bodyMedium, color = Color.White)
            } else {
                LoadingIndicator(Modifier.size(40.dp))
            }
            if (controls) {
                IconButton(
                    onClick = onExit,
                    modifier = Modifier.align(Alignment.TopEnd).safeDrawingPadding().padding(12.dp).clip(CircleShape).background(Color.Black.copy(alpha = 0.45f)),
                ) {
                    Icon(FcIcons.FullscreenExit, contentDescription = "exit fullscreen", tint = Color.White)
                }
            }
        }
    }
}

private fun copy(context: android.content.Context, url: String) {
    val clipboard = context.getSystemService(ClipboardManager::class.java) ?: return
    clipboard.setPrimaryClip(ClipData.newPlainText("framecorder live view", url))
    // android 13+ shows its own "copied" note
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) Toast.makeText(context, "copied", Toast.LENGTH_SHORT).show()
}
