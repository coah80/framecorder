package com.framecorder.app.ui.frame

import android.widget.Toast
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.framecorder.app.sync.RemoteNow
import com.framecorder.app.sync.SyncHub
import com.framecorder.app.sync.Transfer
import com.framecorder.app.sync.reason
import com.framecorder.app.ui.common.AddTile
import com.framecorder.app.ui.common.Chevron
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.GroupLabel
import com.framecorder.app.ui.common.SegmentRow
import com.framecorder.app.ui.common.ShapeIcon
import com.framecorder.app.ui.common.StatusBarBlur
import com.framecorder.app.ui.common.StatusDot
import com.framecorder.app.ui.common.host
import com.framecorder.app.ui.common.isTrouble
import com.framecorder.app.ui.common.length
import com.framecorder.app.ui.common.rate
import com.framecorder.app.ui.common.shortPrint
import com.framecorder.app.ui.common.size
import com.framecorder.app.ui.common.stateColor
import com.framecorder.app.ui.common.stateText
import com.framecorder.app.ui.pair.Pairing
import com.framecorder.app.ui.player.span
import com.framecorder.app.ui.theme.DataStyle
import com.framecorder.app.ui.theme.LocalStatusColors
import com.framecorder.core.Clip
import com.framecorder.core.CoreException
import com.framecorder.core.FrameState
import com.framecorder.core.FrameStatus
import com.framecorder.core.RecordingSettings
import dev.chrisbanes.haze.hazeSource
import dev.chrisbanes.haze.rememberHazeState
import kotlinx.coroutines.launch

@Composable
fun FrameScreen(hub: SyncHub, fingerprint: String?, onDiagnose: (String) -> Unit, onPairAnother: () -> Unit) {
    val frames by hub.frames.collectAsStateWithLifecycle()
    if (frames.isEmpty()) {
        Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface).statusBarsPadding()) {
            Pairing(hub = hub)
        }
        return
    }
    val transfer by hub.transfer.collectAsStateWithLifecycle()
    val clips by hub.clips.collectAsStateWithLifecycle()
    var picked by rememberSaveable { mutableStateOf(fingerprint) }
    val frame = frames.firstOrNull { it.fingerprint == picked } ?: frames.first()
    var menu by rememberSaveable { mutableStateOf(false) }
    var unpairing by rememberSaveable { mutableStateOf(false) }
    val moving = transfer?.takeIf { it.fingerprint == frame.fingerprint }

    // the headset's recording settings, read once it's connected
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val connected = frame.state == FrameState.CONNECTED
    var recording by remember(frame.fingerprint) { mutableStateOf<RecordingSettings?>(null) }
    var asked by remember(frame.fingerprint) { mutableStateOf(false) }
    LaunchedEffect(frame.fingerprint, connected) {
        if (!connected) return@LaunchedEffect
        try {
            recording = hub.recording(frame.fingerprint)
            asked = true
        } catch (e: CoreException) {
            // tried again when it reconnects
        }
    }
    fun change(new: RecordingSettings) {
        val old = recording
        recording = new
        scope.launch {
            try {
                recording = hub.setRecording(frame.fingerprint, new)
            } catch (e: CoreException) {
                recording = old
                Toast.makeText(context, "couldn't change that: ${e.reason()}", Toast.LENGTH_LONG).show()
            }
        }
    }
    val remotes by hub.remotes.collectAsStateWithLifecycle()
    val remote = remotes[frame.fingerprint]?.takeIf { connected }
    var sending by remember { mutableStateOf<String?>(null) }
    val haptics = LocalHapticFeedback.current
    fun command(what: String) {
        if (sending != null) return
        sending = what
        haptics.performHapticFeedback(HapticFeedbackType.Confirm)
        scope.launch {
            try {
                hub.command(frame.fingerprint, what)
                if (what == "clip") Toast.makeText(context, "saved, it'll be here in a moment", Toast.LENGTH_SHORT).show()
            } catch (e: CoreException) {
                haptics.performHapticFeedback(HapticFeedbackType.Reject)
                Toast.makeText(context, e.reason(), Toast.LENGTH_LONG).show()
            } finally {
                sending = null
            }
        }
    }
    val recordingNote = when {
        recording != null -> null
        !connected -> "connect to ${frame.name} to change these."
        asked -> "these can be changed from here once ${frame.name} has the latest framecorder."
        else -> null
    }

    val about by hub.about.collectAsStateWithLifecycle()
    val info = about[frame.fingerprint]?.takeIf { connected }
    val list = rememberLazyListState()
    val haze = rememberHazeState()
    val density = LocalDensity.current
    // the pill takes over once the name has scrolled away
    val collapsed by remember { derivedStateOf { list.firstVisibleItemIndex > 0 || list.firstVisibleItemScrollOffset > with(density) { 100.dp.toPx() } } }
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    val surface = MaterialTheme.colorScheme.surface

    Box(Modifier.fillMaxSize().background(surface)) {
        LazyColumn(
            Modifier.fillMaxSize().hazeSource(haze),
            state = list,
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = top, bottom = 140.dp),
        ) {
            item(key = "header") {
                FrameHeader(frame, info) {
                    Box {
                        IconButton(onClick = { menu = true }) { Icon(FcIcons.More, contentDescription = "more") }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(
                                text = { Text("connection check") },
                                leadingIcon = { Icon(FcIcons.Wifi, contentDescription = null) },
                                onClick = {
                                    menu = false
                                    onDiagnose(frame.fingerprint)
                                },
                            )
                            DropdownMenuItem(
                                text = { Text("try again now") },
                                leadingIcon = { Icon(FcIcons.Retry, contentDescription = null) },
                                onClick = {
                                    menu = false
                                    hub.retryNow()
                                },
                            )
                            DropdownMenuItem(
                                text = { Text("unpair") },
                                leadingIcon = { Icon(FcIcons.Unlink, contentDescription = null) },
                                onClick = {
                                    menu = false
                                    unpairing = true
                                },
                            )
                        }
                    }
                }
            }
            if (frames.size > 1) {
                item {
                    LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(bottom = 12.dp)) {
                        items(frames, key = { it.fingerprint }) { f ->
                            FilterChip(
                                selected = f.fingerprint == frame.fingerprint,
                                onClick = { picked = f.fingerprint },
                                label = { Text(f.name) },
                                leadingIcon = { StatusDot(stateColor(f.state)) },
                            )
                        }
                    }
                }
            }
            item {
                Remote(
                    frame,
                    moving,
                    remote = remote,
                    sending = sending,
                    onCommand = ::command,
                    mic = recording?.mic,
                    onMic = { recording?.let { change(it.copy(mic = !it.mic)) } },
                    onDiagnose = { onDiagnose(frame.fingerprint) },
                )
            }
            item { NextRecording(recording, recordingNote, ::change) }
            item { OnThisPhone(clips.filter { it.host == frame.fingerprint }) }
            item { GroupLabel("this frame") }
            item {
                SegmentRow(
                    0, 3, "connection check",
                    subtitle = "see what's getting in the way",
                    onClick = { onDiagnose(frame.fingerprint) },
                    leading = { Icon(FcIcons.Wifi, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant) },
                    trailing = { Chevron() },
                )
            }
            item {
                Spacer(Modifier.height(2.dp))
                SegmentRow(
                    1, 3, "try again now",
                    subtitle = "instead of waiting for the next look",
                    onClick = hub::retryNow,
                    leading = { Icon(FcIcons.Retry, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant) },
                )
            }
            item {
                Spacer(Modifier.height(2.dp))
                SegmentRow(
                    2, 3, "unpair",
                    subtitle = "what's already here stays · id ${shortPrint(frame.fingerprint)}",
                    titleColor = MaterialTheme.colorScheme.error,
                    onClick = { unpairing = true },
                    leading = { Icon(FcIcons.Unlink, contentDescription = null, tint = MaterialTheme.colorScheme.error) },
                )
            }
            item {
                Spacer(Modifier.height(14.dp))
                SegmentRow(0, 1, "pair another frame", titleColor = MaterialTheme.colorScheme.primary, onClick = onPairAnother, leading = { AddTile() })
            }
        }

        // reaches down past the pill while it's out
        val cover by animateDpAsState(if (collapsed) 96.dp else 28.dp, MaterialTheme.motionScheme.defaultEffectsSpec(), label = "cover")
        val under by remember { derivedStateOf { list.firstVisibleItemIndex > 0 || list.firstVisibleItemScrollOffset > 0 } }
        if (under) {
            StatusBarBlur(haze, top + cover, shown = {
                if (list.firstVisibleItemIndex > 0) 1f else (list.firstVisibleItemScrollOffset / with(density) { 24.dp.toPx() }).coerceIn(0f, 1f)
            })
        }

        AnimatedVisibility(
            visible = collapsed,
            enter = fadeIn() + scaleIn(MaterialTheme.motionScheme.defaultSpatialSpec(), initialScale = 0.8f) +
                slideInVertically(MaterialTheme.motionScheme.defaultSpatialSpec()) { -it },
            exit = fadeOut() + scaleOut(targetScale = 0.8f) + slideOutVertically { -it },
            modifier = Modifier.align(Alignment.TopCenter).statusBarsPadding().padding(top = 8.dp),
        ) {
            FramePill(frame, info, remote, haze, onClick = { scope.launch { list.animateScrollToItem(0) } })
        }

        if (unpairing) {
            AlertDialog(
                onDismissRequest = { unpairing = false },
                title = { Text("unpair ${frame.name}?") },
                text = { Text("it stops sending clips here. what's already on this phone stays, and you can pair again any time.") },
                confirmButton = {
                    TextButton(onClick = {
                        unpairing = false
                        picked = null
                        hub.unpair(frame.fingerprint)
                    }) { Text("unpair") }
                },
                dismissButton = { TextButton(onClick = { unpairing = false }) { Text("keep it") } },
            )
        }
    }
}

/** How it's doing, what's coming over, and the remote, which waits on the headset. */
@Composable
private fun Remote(
    frame: FrameStatus,
    moving: Transfer?,
    remote: RemoteNow?,
    sending: String?,
    onCommand: (String) -> Unit,
    mic: Boolean?,
    onMic: () -> Unit,
    onDiagnose: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    val trouble = frame.state.isTrouble()
    val state = remote?.state
    val recording = state?.recording == true
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(32.dp))
            .background(scheme.surfaceContainerHigh)
            .padding(horizontal = 16.dp, vertical = 18.dp),
        verticalArrangement = Arrangement.spacedBy(18.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            ShapeIcon(
                if (trouble) FcIcons.WifiOff else FcIcons.Wifi,
                shape = MaterialShapes.Sunny,
                size = 40.dp,
                iconSize = 20.dp,
                container = if (trouble) scheme.errorContainer else scheme.secondaryContainer,
                content = if (trouble) scheme.onErrorContainer else scheme.onSecondaryContainer,
            )
            Column(Modifier.weight(1f)) {
                Text("now", style = MaterialTheme.typography.labelMedium, color = scheme.onSurfaceVariant)
                Text(
                    when {
                        recording && state?.running == false -> "recording, paused"
                        recording -> "recording"
                        moving != null -> "syncing"
                        frame.state == FrameState.CONNECTED -> "all synced"
                        frame.state == FrameState.CONNECTING -> "looking for it"
                        else -> stateText(frame)
                    },
                    style = MaterialTheme.typography.titleMedium,
                )
            }
            if (recording && remote != null) {
                RecordingTimer(remote)
            } else if (moving != null) {
                Text(
                    "${(moving.fraction * 100).toInt()}%",
                    style = DataStyle.copy(color = scheme.onPrimaryContainer),
                    modifier = Modifier.clip(CircleShape).background(scheme.primaryContainer).padding(horizontal = 12.dp, vertical = 6.dp),
                )
            }
        }

        if (moving != null) {
            val progress by animateFloatAsState(moving.fraction, MaterialTheme.motionScheme.defaultEffectsSpec(), label = "download")
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                LinearWavyProgressIndicator(progress = { progress }, modifier = Modifier.fillMaxWidth())
                Text(
                    "${size(moving.done)} of ${size(moving.total)}" + (if (moving.bytesPerSecond > 0) " · ${rate(moving.bytesPerSecond)}" else ""),
                    style = DataStyle,
                    color = scheme.onSurfaceVariant,
                    maxLines = 1,
                )
            }
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceAround, verticalAlignment = Alignment.Top) {
            MicControl(mic, onMic)
            val usable = state != null && state.available && state.ready
            RecordControl(recording, enabled = usable && sending == null, onClick = { onCommand(if (recording) "stop" else "record") })
            SaveClipControl(
                label = when {
                    recording -> "in recording"
                    state != null && state.clipSecs == null -> "clips off"
                    else -> "save clip"
                },
                enabled = usable && state?.clipReady == true && !recording && sending == null,
                saving = sending == "clip",
                onClick = { onCommand("clip") },
            )
        }

        if (trouble) {
            FilledTonalButton(onClick = onDiagnose, modifier = Modifier.fillMaxWidth()) { Text("check the connection") }
        } else {
            Text(
                when {
                    state == null -> "record, stop and save clips from here once your frame has the next framecorder update."
                    !state.available -> "framecorder isn't open on the headset. it starts with SteamVR."
                    !state.ready -> "finish setting up framecorder on the headset first."
                    recording && !state.running -> "recording, paused while the framecorder tab is open."
                    recording -> "recording. stop it here or on the headset."
                    state.clipReady -> "keeping the last ${span((state.clipSecs ?: 30u).toLong() * 1000)}, ready to save."
                    state.clipSecs == null -> "clips are off, so there's nothing to save. record instead, or turn them on."
                    else -> "clips are starting up..."
                },
                style = MaterialTheme.typography.bodySmall,
                color = scheme.onSurfaceVariant,
                modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(scheme.surfaceContainer).padding(horizontal = 14.dp, vertical = 10.dp),
            )
        }
    }
}

@Composable
private fun Control(label: String, icon: ImageVector, size: Dp, shape: androidx.compose.ui.graphics.Shape) {
    val scheme = MaterialTheme.colorScheme
    Column(
        Modifier.width(88.dp).semantics {
            contentDescription = label
            disabled()
        },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Box(
            Modifier.padding(top = 16.dp).size(size).clip(shape).background(scheme.surfaceContainerHighest),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, contentDescription = null, tint = scheme.onSurface.copy(alpha = 0.38f))
        }
        Text(label, style = MaterialTheme.typography.labelMedium, color = scheme.onSurfaceVariant.copy(alpha = 0.6f), maxLines = 1)
    }
}

/** The mic for the next recording: a rounded square when on, a circle when off. */
@Composable
private fun MicControl(on: Boolean?, onToggle: () -> Unit) {
    if (on == null) {
        Control("mic", FcIcons.Mic, size = 64.dp, shape = RoundedCornerShape(20.dp))
        return
    }
    val scheme = MaterialTheme.colorScheme
    val corner by animateDpAsState(if (on) 20.dp else 32.dp, MaterialTheme.motionScheme.fastSpatialSpec(), label = "mic shape")
    val container by animateColorAsState(if (on) scheme.secondaryContainer else scheme.surfaceContainerHighest, label = "mic")
    val content by animateColorAsState(if (on) scheme.onSecondaryContainer else scheme.onSurfaceVariant, label = "mic ink")
    Column(Modifier.width(88.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(
            Modifier
                .padding(top = 16.dp)
                .size(64.dp)
                .clip(RoundedCornerShape(corner))
                .background(container)
                .toggleable(value = on, role = Role.Switch, onValueChange = { onToggle() }),
            contentAlignment = Alignment.Center,
        ) {
            Icon(if (on) FcIcons.Mic else FcIcons.MicOff, contentDescription = null, tint = content)
        }
        Text(if (on) "mic on" else "mic off", style = MaterialTheme.typography.labelMedium, color = scheme.onSurfaceVariant, maxLines = 1)
    }
}

/** A red circle; while recording, a rounded square with a stop in it. */
@Composable
private fun RecordControl(recording: Boolean, enabled: Boolean, onClick: () -> Unit) {
    val status = LocalStatusColors.current
    val spring = MaterialTheme.motionScheme.defaultSpatialSpec<Dp>()
    val corner by animateDpAsState(if (recording) 28.dp else 48.dp, spring, label = "record shape")
    val stop by animateDpAsState(if (recording) 30.dp else 0.dp, spring, label = "stop")
    val container by animateColorAsState(
        when {
            recording -> status.recContainer
            enabled -> status.rec
            else -> status.rec.copy(alpha = 0.32f)
        },
        label = "record",
    )
    Column(Modifier.width(120.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(
            Modifier
                .size(96.dp)
                .then(if (enabled) Modifier.shadow(6.dp, RoundedCornerShape(corner)) else Modifier)
                .clip(RoundedCornerShape(corner))
                .background(container)
                .clickable(enabled = enabled, role = Role.Button, onClickLabel = if (recording) "stop recording" else "start recording", onClick = onClick),
            contentAlignment = Alignment.Center,
        ) {
            Box(Modifier.size(stop).clip(RoundedCornerShape(8.dp)).background(status.onRecContainer))
        }
        Text(
            if (recording) "stop" else "record",
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurface.copy(alpha = if (enabled || recording) 1f else 0.6f),
        )
    }
}

@Composable
private fun SaveClipControl(label: String, enabled: Boolean, saving: Boolean, onClick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    val container by animateColorAsState(if (enabled || saving) scheme.primaryContainer else scheme.surfaceContainerHighest, label = "clip")
    val content by animateColorAsState(if (enabled || saving) scheme.onPrimaryContainer else scheme.onSurface.copy(alpha = 0.38f), label = "clip ink")
    Column(Modifier.width(88.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(
            Modifier
                .padding(top = 16.dp)
                .size(64.dp)
                .clip(RoundedCornerShape(20.dp))
                .background(container)
                .clickable(enabled = enabled, role = Role.Button, onClickLabel = "save the last moments as a clip", onClick = onClick),
            contentAlignment = Alignment.Center,
        ) {
            if (saving) LoadingIndicator(Modifier.size(36.dp), color = content) else Icon(FcIcons.SaveClip, contentDescription = null, tint = content)
        }
        Text(label, style = MaterialTheme.typography.labelMedium, color = scheme.onSurfaceVariant.copy(alpha = if (enabled) 1f else 0.6f), maxLines = 1)
    }
}

/** How long it's been recording, counting along. */
@Composable
private fun RecordingTimer(remote: RemoteNow) {
    val status = LocalStatusColors.current
    Row(
        Modifier.clip(CircleShape).background(status.recContainer).padding(horizontal = 12.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        StatusDot(status.rec)
        Text(length(rememberElapsed(remote) / 1000.0), style = DataStyle.copy(color = status.onRecContainer))
    }
}

/** What's come over from this frame, clips and recordings side by side. */
@Composable
private fun OnThisPhone(clips: List<Clip>) {
    val scheme = MaterialTheme.colorScheme
    val clipBytes = clips.filter { it.kind == "clip" }.sumOf { it.size }
    val total = clips.sumOf { it.size }
    Column(
        Modifier
            .padding(top = 12.dp)
            .fillMaxWidth()
            .clip(RoundedCornerShape(24.dp))
            .background(scheme.surfaceContainerLow)
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row {
            Text("on this phone", style = MaterialTheme.typography.titleSmall, modifier = Modifier.weight(1f))
            Text(
                if (clips.isEmpty()) "nothing yet" else "${clips.size} ${if (clips.size == 1) "video" else "videos"} · ${size(total)}",
                style = DataStyle,
                color = scheme.onSurfaceVariant,
            )
        }
        if (total > 0) {
            val share = (clipBytes.toFloat() / total).coerceIn(0f, 1f)
            Row(Modifier.fillMaxWidth().height(8.dp), horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                if (share > 0f) Box(Modifier.weight(share).fillMaxHeight().clip(CircleShape).background(scheme.primary))
                if (share < 1f) Box(Modifier.weight(1f - share).fillMaxHeight().clip(CircleShape).background(scheme.onSurfaceVariant))
            }
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                if (clipBytes > 0) Legend(scheme.primary, "clips ${size(clipBytes)}")
                if (total > clipBytes) Legend(scheme.onSurfaceVariant, "recordings ${size(total - clipBytes)}")
            }
        }
    }
}

@Composable
private fun Legend(color: Color, text: String) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        StatusDot(color)
        Text(text, style = DataStyle, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
