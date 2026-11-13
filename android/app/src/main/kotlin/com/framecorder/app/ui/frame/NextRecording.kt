package com.framecorder.app.ui.frame

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
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
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
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
/**
 * Both eyes blended into one level view, the way SteamVR's own view does it,
 * the dominant eye filling the middle. Older headsets can't pick that eye.
 */
private val BlendedEye = "both" to "both eyes blended"
private val Dominant = listOf("both:left" to "both, left dominant", "both:right" to "both, right dominant")

/**
 * The eyes on offer: the blend only for a headset whose framecorder can do it,
 * with a dominant eye once it [dominant] can pick one. A blend with a dominant
 * eye is keyed "both:left" or "both:right", see [eyeKey] and [eyePatch].
 */
fun eyeChoices(blend: Boolean, dominant: Boolean): List<Pair<String, String>> = when {
    !blend -> Eyes
    dominant -> Eyes + Dominant
    else -> Eyes + BlendedEye
}

/** The choice that's on for [eye] and [dominant], as [eyeChoices] keys them. */
fun eyeKey(eye: String, dominant: String?): String = if (eye == "both" && dominant != null) "both:$dominant" else eye

/** What picking [key] changes on the headset. */
fun eyePatch(key: String): RecordingPatch = key.split(':').let { RecordingPatch(eye = it[0], dominant = it.getOrNull(1)) }
/** 0 is clips off, like the desktop app's. */
private val Lengths = listOf(0u to "off", 15u to "15 s", 30u to "30 s", 60u to "1 min", 120u to "2 min")

/** Without the panels unlocked, the recorder records SteamVR's view: 16:9, the left eye. */
private const val PANELS_NOTE = "square, vertical, both eyes and picking an eye need the panels unlocked on the headset."
private val Qualities = listOf("standard" to "standard", "high" to "high", "max" to "max")
private val Rates = listOf("auto" to "auto", "60" to "60 fps", "30" to "30 fps")
private val Codecs = listOf("hevc" to "HEVC", "h264" to "H.264")
private val Formats = listOf("mp4" to "hybrid MP4", "mkv" to "MKV")

/** What the headset uses when nothing's been read from it yet. */
private val Defaults = RecordingSettings("wide", "high", "auto", gameAudio = true, mic = true, clips = true, clipSecs = 30u, eye = null, dominant = null, panels = null, splitAudio = null, micSource = null, mics = null, codec = null, format = null, remux = null, folder = null, folders = null)

private fun mbps(quality: String) = when (quality) {
    "standard" -> 20
    "max" -> 80
    else -> 40
}

/**
 * The headset's mics and the picked one, which is none of them when it's been
 * unplugged. Older headsets don't list them.
 */
private fun micChoices(s: RecordingSettings): Pair<List<Pair<String, String>>, String>? {
    val mics = s.mics ?: return null
    val picked = s.micSource.orEmpty()
    val gone = picked.isNotEmpty() && mics.none { it.name == picked }
    if (mics.isEmpty() && !gone) return null
    return mics.map { it.name to it.label } to picked.ifEmpty { mics.firstOrNull { it.default }?.name.orEmpty() }
}

private fun audio(s: RecordingSettings) = when {
    s.gameAudio && s.mic && s.splitAudio == true -> "game + mic · split tracks"
    s.gameAudio && s.mic -> "game + mic"
    s.gameAudio -> "game audio"
    s.mic -> "mic only"
    else -> "no sound"
}

/**
 * The headset's recording settings: shape and clip length right here, the
 * rest in a sheet. [settings] is null until the frame has said what they are.
 * [blend] is whether its framecorder can blend both eyes into one view.
 */
@Composable
fun NextRecording(settings: RecordingSettings?, note: String?, blend: Boolean, onChange: (RecordingPatch) -> Unit, modifier: Modifier = Modifier) {
    var more by rememberSaveable { mutableStateOf(false) }
    val enabled = settings != null
    val s = settings ?: Defaults
    // an older headset doesn't say which eye; one without the panels only records 16:9 of the left
    val eyes = s.eye != null
    val panels = s.panels != false
    val rows = if (eyes) 4 else 3
    val eyeChoices = eyeChoices(blend, s.dominant != null)
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
                    Choices(eyeChoices, if (panels) eyeKey(s.eye ?: "left", s.dominant) else "left", enabled && panels, lines = if (blend) 2 else 1) { onChange(eyePatch(it)) }
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
                subtitle = listOfNotNull(s.quality, Rates.first { it.first == s.fps }.second, Codecs.firstOrNull { it.first == s.codec }?.second, Formats.firstOrNull { it.first == s.format }?.second, audio(s)).joinToString(" · "),
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
                val mics = micChoices(settings)
                // older headsets can't pick a codec
                val codec = settings.codec
                val format = settings.format
                val folders = settings.folders
                val sound = 2 + listOfNotNull(codec, format, folders).size
                val groups = sound + if (mics != null) 2 else 1
                Group(0, groups, "quality · ${mbps(settings.quality)} Mbit/s") {
                    Choices(Qualities, settings.quality, true) { onChange(RecordingPatch(quality = it)) }
                }
                Group(1, groups, "frame rate") {
                    Choices(Rates, settings.fps, true) { onChange(RecordingPatch(fps = it)) }
                }
                if (codec != null) {
                    Group(2, groups, "codec") {
                        Choices(Codecs, codec, true) { onChange(RecordingPatch(codec = it)) }
                    }
                }
                if (folders != null) {
                    // a picked drive that's been taken out records to videos meanwhile
                    val options = folders.map { it.path to if (it.pluggedIn) it.name else "${it.name} isn't in · saving to Videos" }
                    Group(2 + listOfNotNull(codec).size, groups, "folder") {
                        Dropdown(options, settings.folder.orEmpty(), true) { onChange(RecordingPatch(folder = it)) }
                    }
                }
                if (format != null) {
                    Group(sound - 1, groups, "format") {
                        Choices(Formats, format, true) { onChange(RecordingPatch(format = it)) }
                        // only an mkv gets remuxed
                        val remux = settings.remux
                        if (format == "mkv" && remux != null) {
                            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                                Text("remux recordings to MP4", style = MaterialTheme.typography.titleSmall.copy(fontSize = 15.sp), modifier = Modifier.weight(1f))
                                Switch(
                                    checked = remux,
                                    onCheckedChange = { onChange(RecordingPatch(remux = it)) },
                                    thumbContent = if (remux) ({ Icon(FcIcons.Check, contentDescription = null, modifier = Modifier.size(SwitchDefaults.IconSize)) }) else null,
                                )
                            }
                        }
                    }
                }
                Column(
                    Modifier.fillMaxWidth().clip(segmentShape(sound, groups, outer = 20.dp)).background(MaterialTheme.colorScheme.surfaceContainer),
                ) {
                    Toggle("game audio", settings.gameAudio) { onChange(RecordingPatch(gameAudio = it)) }
                    Toggle("mic", settings.mic) { onChange(RecordingPatch(mic = it)) }
                    // older headsets can't split, and splitting needs both
                    settings.splitAudio?.let { split ->
                        val both = settings.gameAudio && settings.mic
                        Toggle("split tracks", both && split, enabled = both) { onChange(RecordingPatch(splitAudio = it)) }
                    }
                }
                if (mics != null) {
                    Group(sound + 1, groups, "mic") {
                        Dropdown(mics.first, mics.second, settings.mic) { onChange(RecordingPatch(micSource = it)) }
                    }
                }
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

/** A connected button group with one picked. [lines] lets a long label wrap instead of being cut. */
@Composable
private fun <T> Choices(
    options: List<Pair<T, String>>,
    picked: T,
    enabled: Boolean,
    tall: Boolean = false,
    lines: Int = 1,
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
                modifier = Modifier.weight(1f).height(if (tall || lines > 1) 60.dp else 40.dp).semantics { role = Role.RadioButton },
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
                    Text(
                        label,
                        style = if (tall || lines > 1) MaterialTheme.typography.labelMedium else MaterialTheme.typography.labelLarge,
                        maxLines = lines,
                        textAlign = TextAlign.Center,
                    )
                }
            }
        }
    }
}

/** A pick from a list that can run long, like the headset's mics: the pick in a field that opens the list. With nothing else to pick the field just shows the value. */
@Composable
private fun Dropdown(options: List<Pair<String, String>>, picked: String, enabled: Boolean, onPick: (String) -> Unit) {
    var open by rememberSaveable { mutableStateOf(false) }
    var width by remember { mutableIntStateOf(0) }
    val label = options.firstOrNull { it.first == picked }?.second ?: "not connected"
    val fixed = options.size < 2 && options.any { it.first == picked }
    Box {
        Row(
            Modifier
                .fillMaxWidth()
                .height(48.dp)
                .onSizeChanged { width = it.width }
                .clip(RoundedCornerShape(14.dp))
                .background(MaterialTheme.colorScheme.surfaceContainerHighest)
                .clickable(enabled = enabled && !fixed, role = Role.DropdownList) { open = true }
                .alpha(if (enabled) 1f else 0.4f)
                .padding(start = 16.dp, end = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(label, style = MaterialTheme.typography.labelLarge, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            if (!fixed) Icon(FcIcons.ChevronDown, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        DropdownMenu(
            expanded = open && enabled && !fixed,
            onDismissRequest = { open = false },
            modifier = Modifier.width(with(LocalDensity.current) { width.toDp() }),
        ) {
            options.forEach { (value, name) ->
                val on = value == picked
                DropdownMenuItem(
                    text = { Text(name, maxLines = 1, overflow = TextOverflow.Ellipsis, color = if (on) MaterialTheme.colorScheme.primary else Color.Unspecified) },
                    trailingIcon = if (on) {
                        { Icon(FcIcons.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary) }
                    } else {
                        null
                    },
                    onClick = {
                        open = false
                        if (!on) onPick(value)
                    },
                )
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
private fun Toggle(title: String, on: Boolean, enabled: Boolean = true, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = MaterialTheme.typography.titleSmall.copy(fontSize = 15.sp), modifier = Modifier.weight(1f))
        Spacer(Modifier.size(12.dp))
        Switch(
            checked = on,
            enabled = enabled,
            onCheckedChange = onChange,
            thumbContent = if (on) ({ Icon(FcIcons.Check, contentDescription = null, modifier = Modifier.size(SwitchDefaults.IconSize)) }) else null,
        )
    }
}
