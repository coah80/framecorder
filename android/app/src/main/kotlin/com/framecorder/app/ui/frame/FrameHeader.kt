package com.framecorder.app.ui.frame

import android.os.SystemClock
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.framecorder.app.sync.RemoteNow
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.GlassPill
import com.framecorder.app.ui.common.PillRule
import com.framecorder.app.ui.common.StatusDot
import com.framecorder.app.ui.common.dotted
import com.framecorder.app.ui.common.glass
import com.framecorder.app.ui.common.host
import com.framecorder.app.ui.common.length
import com.framecorder.app.ui.common.size
import com.framecorder.app.ui.common.stateColor
import com.framecorder.app.ui.common.stateText
import com.framecorder.app.ui.theme.DataStyle
import com.framecorder.app.ui.theme.LocalStatusColors
import com.framecorder.app.ui.theme.SpaceGrotesk
import com.framecorder.core.FrameInfo
import com.framecorder.core.FrameStatus
import dev.chrisbanes.haze.HazeState
import kotlinx.coroutines.delay

/** The name big, how it's connected, and how the headset's doing, before the page scrolls. */
@Composable
fun FrameHeader(frame: FrameStatus, info: FrameInfo?, actions: @Composable RowScope.() -> Unit) {
    val scheme = MaterialTheme.colorScheme
    Column(Modifier.fillMaxWidth().padding(start = 4.dp, bottom = 18.dp)) {
        Row(
            Modifier.fillMaxWidth().height(56.dp).offset(x = 12.dp),
            horizontalArrangement = Arrangement.End,
            verticalAlignment = Alignment.CenterVertically,
            content = actions,
        )
        Text(dotted(frame.name), style = MaterialTheme.typography.displaySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Row(Modifier.padding(top = 2.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            StatusDot(stateColor(frame.state))
            Text(
                "steam frame · ${stateText(frame)} · ${host(frame.addr)}",
                style = MaterialTheme.typography.bodyMedium,
                color = scheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        AnimatedVisibility(
            info != null && (info.batteryPercent != null || info.freeBytes != null),
            enter = fadeIn() + expandVertically(),
            exit = fadeOut() + shrinkVertically(),
        ) {
            FlowRow(Modifier.padding(top = 14.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                info?.batteryPercent?.let { percent ->
                    val p = percent.toInt()
                    Chip(Modifier.semantics { contentDescription = "battery $p%" + if (info.charging) ", charging" else "" }) {
                        BatteryGlyph(p, info.charging, height = 20.dp)
                    }
                }
                info?.freeBytes?.let { free ->
                    Chip {
                        Icon(FcIcons.Storage, contentDescription = null, tint = scheme.onSurfaceVariant, modifier = Modifier.size(18.dp))
                        Text("${size(free.toLong())} free", style = DataStyle, color = scheme.onSurface)
                    }
                }
            }
        }
    }
}

@Composable
private fun Chip(modifier: Modifier = Modifier, content: @Composable RowScope.() -> Unit) {
    Row(
        modifier
            .height(36.dp)
            .clip(CircleShape)
            .background(MaterialTheme.colorScheme.surfaceContainerHigh)
            .padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        content = content,
    )
}

/**
 * The frame once the page has scrolled: frosted glass in the middle up top
 * with its name, its state, and the battery or how long it's been
 * recording. Tapping it goes back to the top.
 */
@Composable
fun FramePill(frame: FrameStatus, info: FrameInfo?, remote: RemoteNow?, haze: HazeState, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val status = LocalStatusColors.current
    val recording = remote?.state?.recording == true
    GlassPill(
        haze,
        onClick = onClick,
        onClickLabel = "back to the top",
        modifier = modifier.semantics { contentDescription = "${frame.name}, ${stateText(frame)}" },
        padding = PaddingValues(start = 10.dp, end = 20.dp),
    ) {
        Box(Modifier.size(40.dp)) {
            Box(Modifier.size(40.dp).clip(CircleShape).background(scheme.surfaceContainerHighest), contentAlignment = Alignment.Center) {
                Icon(FcIcons.Headset, contentDescription = null, tint = scheme.onSurfaceVariant, modifier = Modifier.size(22.dp))
            }
            StatusDot(stateColor(frame.state), Modifier.align(Alignment.BottomEnd).offset(2.dp, 2.dp), size = 10.dp, ring = scheme.surfaceContainerHigh)
        }
        Text(
            frame.name,
            style = MaterialTheme.typography.titleMedium.copy(fontSize = 17.sp, fontWeight = FontWeight.SemiBold),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.widthIn(max = 170.dp),
        )
        when {
            recording && remote != null -> {
                PillRule()
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    StatusDot(status.rec, size = 10.dp)
                    Text(length(rememberElapsed(remote) / 1000.0), style = DataStyle.copy(fontSize = 15.sp), color = scheme.onSurface)
                }
            }
            info?.batteryPercent != null -> {
                PillRule()
                BatteryGlyph(info.batteryPercent!!.toInt(), info.charging, height = 20.dp)
            }
        }
    }
}

/** How long it's been recording, counting along. */
@Composable
fun rememberElapsed(remote: RemoteNow): Long {
    val now by produceState(SystemClock.elapsedRealtime(), remote) {
        while (true) {
            value = SystemClock.elapsedRealtime()
            delay(250)
        }
    }
    return remote.elapsedMs(now)
}

private val bolt = PathParser().parsePathString("M13.5 2.5L5.5 13.5h5.5l-1 8 8-11h-5.5z").toPath()

/**
 * A battery with the percentage inside, the way phones show their own: it
 * fills to the level, green while charging (with a bolt by the number),
 * red when it's nearly out. The number reads dark over the fill and light
 * past it.
 */
@Composable
fun BatteryGlyph(percent: Int, charging: Boolean, modifier: Modifier = Modifier, height: Dp = 18.dp) {
    val scheme = MaterialTheme.colorScheme
    val status = LocalStatusColors.current
    val level by animateFloatAsState(percent.coerceIn(0, 100) / 100f, MaterialTheme.motionScheme.slowEffectsSpec(), label = "battery")
    val fill = when {
        charging -> status.ok
        percent <= 15 -> scheme.error
        else -> scheme.onSurface
    }
    val track = scheme.onSurface.copy(alpha = 0.24f)
    val onFill = scheme.surface
    val onTrack = scheme.onSurface
    val measurer = rememberTextMeasurer()
    val style = remember(height) {
        DataStyle.copy(fontFamily = SpaceGrotesk, fontWeight = FontWeight.Bold, fontSize = (height.value * 0.62f).sp, lineHeight = (height.value * 0.62f).sp)
    }
    val label = measurer.measure(percent.coerceIn(0, 100).toString(), style)
    Canvas(modifier.size(width = height * if (charging) 2.5f else 2.1f, height = height)) {
        val cap = size.height * 0.14f
        val gap = size.height * 0.07f
        val body = Size(size.width - cap - gap, size.height)
        val corner = CornerRadius(size.height * 0.34f)
        drawRoundRect(track, size = body, cornerRadius = corner)
        val outline = Path().apply { addRoundRect(RoundRect(0f, 0f, body.width, body.height, corner)) }
        val filled = body.width * level
        clipPath(outline) { drawRect(fill, size = Size(filled, body.height)) }
        drawRoundRect(
            if (level >= 0.999f) fill else track,
            topLeft = Offset(body.width + gap, size.height * 0.32f),
            size = Size(cap, size.height * 0.36f),
            cornerRadius = CornerRadius(cap / 2),
        )

        // the number, with a bolt in front of it while charging
        val boltSize = size.height * 0.62f
        val boltGap = size.height * 0.08f
        val content = label.size.width + if (charging) boltSize + boltGap else 0f
        var x = (body.width - content) / 2
        val y = (body.height - label.size.height) / 2
        fun ink(draw: (Color) -> Unit) {
            clipRect(right = filled) { draw(onFill) }
            clipRect(left = filled) { draw(onTrack) }
        }
        if (charging) {
            val at = x
            ink { color ->
                translate(at, (body.height - boltSize) / 2) {
                    scale(boltSize / 24f, pivot = Offset.Zero) { drawPath(bolt, color) }
                }
            }
            x += boltSize + boltGap
        }
        val textAt = Offset(x, y)
        ink { color -> drawText(label, color = color, topLeft = textAt) }
    }
}
