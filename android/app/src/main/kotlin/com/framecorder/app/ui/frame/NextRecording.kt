package com.framecorder.app.ui.frame

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.ToggleButtonDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.framecorder.app.ui.common.Chevron
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.GroupLabel
import com.framecorder.app.ui.common.SegmentRow
import com.framecorder.app.ui.common.dotted
import com.framecorder.app.ui.common.segmentShape
import com.framecorder.core.RecordingPatch
import com.framecorder.core.RecordingSettings

private val Shapes = listOf("wide" to "16:9", "square" to "1:1", "tall" to "9:16", "both" to "both eyes")
private val Eyes = listOf("left" to "left eye", "right" to "right eye")
/** 0 is clips off, like the desktop app's. */
private val Lengths = listOf(0u to "off", 15u to "15 s", 30u to "30 s", 60u to "1 min", 120u to "2 min")

/** Without the panels unlocked, the recorder records SteamVR's view: 16:9, the left eye. */
private const val PANELS_NOTE = "square, vertical, both eyes and the eye choice need the panels unlocked on the headset; until then it records SteamVR's view."
private val Qualities = listOf("standard" to "standard", "high" to "high", "max" to "max")
private val Rates = listOf("auto" to "auto", "60" to "60 fps", "30" to "30 fps")

/** What the headset uses when nothing's been read from it yet. */
private val Defaults = RecordingSettings("wide", "high", "auto", gameAudio = true, mic = true, clips = true, clipSecs = 30u, eye = null, panels = null)

private fun mbps(quality: String) = when (quality) {
    "standard" -> 20
    "max" -> 80
    else -> 40
}

private fun audio(s: RecordingSettings) = when {
    s.gameAudio && s.mic -> "game + mic"
    s.gameAudio -> "game audio"
    s.mic -> "mic only"
    else -> "no sound"
}

/**
 * The headset's recording settings: shape and clip length right here, the
 * rest in a sheet. [settings] is null until the frame has said what they are.
 */
@Composable
fun NextRecording(settings: RecordingSettings?, note: String?, onChange: (RecordingPatch) -> Unit, modifier: Modifier = Modifier) {
    var more by rememberSaveable { mutableStateOf(false) }
    val enabled = settings != null
    val s = settings ?: Defaults
    // an older headset doesn't say which eye; one without the panels only records 16:9 of the left
    val eyes = s.eye != null
    val panels = s.panels != false
    val rows = if (eyes) 4 else 3
    Column(modifier) {
        GroupLabel("next recording")
        Column(Modifier.alpha(if (enabled) 1f else 0.5f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Group(0, rows, "shape") {
                Choices(
                    Shapes, if (panels) s.shape else "wide", enabled,
                    tall = true,
                    glyph = { ShapeGlyph(it) },
                    allowed = { panels || it == "wide" },
                ) { onChange(RecordingPatch(shape = it)) }
            }
            if (eyes) {
                Group(1, rows, "eye") {
                    Choices(Eyes, if (panels) s.eye ?: "left" else "left", enabled && panels) { onChange(RecordingPatch(eye = it)) }
                }
            }
            Group(rows - 2, rows, "clip length") {
                Choices(Lengths, if (s.clips) s.clipSecs else 0u, enabled) { secs ->
                    onChange(
                        when {
                            secs == 0u -> RecordingPatch(clips = false)
                            // the length it remembers: just turn clips back on
                            !s.clips && secs == s.clipSecs -> RecordingPatch(clips = true)
                            s.clips -> RecordingPatch(clipSecs = secs)
                            else -> RecordingPatch(clipSecs = secs, clips = true)
                        },
                    )
                }
            }
            SegmentRow(
                rows - 1, rows, "quality, frame rate, audio",
                subtitle = "${s.quality} · ${Rates.first { it.first == s.fps }.second} · ${audio(s)}",
                onClick = if (enabled) ({ more = true }) else null,
                trailing = { Chevron() },
            )
        }
        if (note != null || (enabled && !panels)) {
            Text(
                note ?: PANELS_NOTE,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 10.dp),
            )
        }
    }

    if (more && settings != null) {
        ModalBottomSheet(onDismissRequest = { more = false }, containerColor = MaterialTheme.colorScheme.surfaceContainerLow) {
            Column(
                Modifier.navigationBarsPadding().padding(start = 16.dp, end = 16.dp, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Text(dotted("next recording"), style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(start = 8.dp, bottom = 14.dp))
                Group(0, 3, "quality · ${mbps(settings.quality)} Mbit/s") {
                    Choices(Qualities, settings.quality, true) { onChange(RecordingPatch(quality = it)) }
                }
                Group(1, 3, "frame rate") {
                    Choices(Rates, settings.fps, true) { onChange(RecordingPatch(fps = it)) }
                }
                Column(
                    Modifier.fillMaxWidth().clip(segmentShape(2, 3, outer = 20.dp)).background(MaterialTheme.colorScheme.surfaceContainer),
                ) {
                    Toggle("game audio", "what you hear in the headset", settings.gameAudio) { onChange(RecordingPatch(gameAudio = it)) }
                    Toggle("mic", "your voice, mixed in", settings.mic) { onChange(RecordingPatch(mic = it)) }
                }
                Text(
                    "these take effect from the frame's next recording.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 8.dp, top = 12.dp),
                )
            }
        }
    }
}

@Composable
private fun Group(index: Int, count: Int, label: String, content: @Composable () -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(segmentShape(index, count, outer = 20.dp))
            .background(MaterialTheme.colorScheme.surfaceContainer)
            .padding(start = 14.dp, end = 14.dp, top = 12.dp, bottom = 14.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(label, style = MaterialTheme.typography.bodySmall.copy(fontSize = 13.sp), color = MaterialTheme.colorScheme.onSurfaceVariant)
        content()
    }
}

/** A connected button group with one picked. */
@Composable
private fun <T> Choices(
    options: List<Pair<T, String>>,
    picked: T,
    enabled: Boolean,
    tall: Boolean = false,
    glyph: (@Composable (T) -> Unit)? = null,
    allowed: (T) -> Boolean = { true },
    onPick: (T) -> Unit,
) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
        options.forEachIndexed { i, (value, label) ->
            ToggleButton(
                checked = value == picked,
                onCheckedChange = { if (value != picked) onPick(value) },
                enabled = enabled && allowed(value),
                modifier = Modifier.weight(1f).height(if (tall) 60.dp else 40.dp).semantics { role = Role.RadioButton },
                shapes = when (i) {
                    0 -> ButtonGroupDefaults.connectedLeadingButtonShapes()
                    options.lastIndex -> ButtonGroupDefaults.connectedTrailingButtonShapes()
                    else -> ButtonGroupDefaults.connectedMiddleButtonShapes()
                },
                colors = ToggleButtonDefaults.colors(
                    containerColor = MaterialTheme.colorScheme.surfaceContainerHighest,
                    contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                ),
                contentPadding = PaddingValues(horizontal = 4.dp),
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(5.dp)) {
                    glyph?.invoke(value)
                    Text(label, style = if (tall) MaterialTheme.typography.labelMedium else MaterialTheme.typography.labelLarge, maxLines = 1)
                }
            }
        }
    }
}

/** The outline of the picture each shape makes. */
@Composable
private fun ShapeGlyph(shape: String) {
    Row(Modifier.height(20.dp), horizontalArrangement = Arrangement.spacedBy(2.dp), verticalAlignment = Alignment.CenterVertically) {
        when (shape) {
            "square" -> Outline(18, 18)
            "tall" -> Outline(13, 20)
            "both" -> {
                Outline(13, 16)
                Outline(13, 16)
            }
            else -> Outline(26, 16)
        }
    }
}

@Composable
private fun Outline(width: Int, height: Int) {
    Box(Modifier.size(width.dp, height.dp).border(2.dp, LocalContentColor.current, RoundedCornerShape(4.dp)))
}

@Composable
private fun Toggle(title: String, subtitle: String, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleSmall.copy(fontSize = 15.sp))
            Text(subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Spacer(Modifier.size(12.dp))
        Switch(
            checked = on,
            onCheckedChange = onChange,
            thumbContent = if (on) ({ Icon(FcIcons.Check, contentDescription = null, modifier = Modifier.size(SwitchDefaults.IconSize)) }) else null,
        )
    }
}
