package com.framecorder.app.ui.common

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.LinearOutSlowInEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandHorizontally
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkHorizontally
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.FloatingToolbarDefaults
import androidx.compose.material3.HorizontalFloatingToolbar
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.graphics.shapes.RoundedPolygon
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.framecorder.app.sync.MediaGallery
import com.framecorder.app.ui.Screen
import com.framecorder.app.ui.theme.LocalStatusColors
import com.framecorder.core.Clip
import com.framecorder.core.FrameState
import com.framecorder.core.FrameStatus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.withContext

/** A title with the accent-colored dot after it, like the wordmark. */
@Composable
fun dotted(text: String) = buildAnnotatedString {
    append(text)
    withStyle(SpanStyle(color = MaterialTheme.colorScheme.primary)) { append(".") }
}

@Composable
fun stateColor(state: FrameState): Color = when (state) {
    FrameState.CONNECTED -> LocalStatusColors.current.ok
    FrameState.CONNECTING -> MaterialTheme.colorScheme.outline
    else -> MaterialTheme.colorScheme.error
}

fun stateText(status: FrameStatus): String = when (status.state) {
    FrameState.CONNECTED -> "connected"
    FrameState.CONNECTING -> "looking for it..."
    FrameState.UNREACHABLE -> "can't reach it"
    FrameState.UNPAIRED -> "doesn't know this phone anymore"
    FrameState.WRONG_FINGERPRINT -> "something else answered"
    FrameState.FULL -> "this phone is out of space"
}

fun FrameState.isTrouble() = this != FrameState.CONNECTED && this != FrameState.CONNECTING

/** A colored dot. A [ring] goes around the outside, the color of what it sits on, so it reads as cut out. */
@Composable
fun StatusDot(color: Color, modifier: Modifier = Modifier, size: Dp = 8.dp, ring: Color? = null) {
    val fill by animateColorAsState(color, label = "dot")
    Box(
        modifier.size(if (ring != null) size + 4.dp else size).drawBehind {
            if (ring != null) drawCircle(ring)
            drawCircle(fill, radius = size.toPx() / 2)
        },
    )
}

/** A ring that keeps growing out of a dot and fading, for something that's busy. */
@Composable
private fun Pulse(color: Color, modifier: Modifier = Modifier) {
    val t = rememberInfiniteTransition(label = "pulse")
    val p by t.animateFloat(0f, 1f, infiniteRepeatable(tween(1600, easing = LinearOutSlowInEasing)), label = "pulse")
    Box(
        modifier.size(8.dp).graphicsLayer {
            scaleX = 1f + p * 1.6f
            scaleY = 1f + p * 1.6f
            alpha = (1f - p) * 0.55f
        }.drawBehind { drawCircle(color) },
    )
}

/** An icon on one of the Material shapes, a cookie by default. */
@Composable
fun ShapeIcon(
    icon: ImageVector,
    modifier: Modifier = Modifier,
    shape: RoundedPolygon = MaterialShapes.Cookie9Sided,
    size: Dp = 48.dp,
    iconSize: Dp = 24.dp,
    container: Color = MaterialTheme.colorScheme.primaryContainer,
    content: Color = MaterialTheme.colorScheme.onPrimaryContainer,
) {
    Box(modifier.size(size).clip(shape.toShape()).background(container), contentAlignment = Alignment.Center) {
        Icon(icon, contentDescription = null, tint = content, modifier = Modifier.size(iconSize))
    }
}

/** A paired Frame in a list: the headset in a circle, its state as a dot on the edge. */
@Composable
fun FrameAvatar(state: FrameState, ring: Color = MaterialTheme.colorScheme.surfaceContainer) {
    Box(Modifier.size(40.dp)) {
        Box(
            Modifier.fillMaxSize().clip(CircleShape).background(MaterialTheme.colorScheme.surfaceContainerHighest),
            contentAlignment = Alignment.Center,
        ) {
            Icon(FcIcons.Headset, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(20.dp))
        }
        StatusDot(stateColor(state), Modifier.align(Alignment.BottomEnd), size = 12.dp, ring = ring)
    }
}

/** A tinted square with an icon, for rows that add something. */
@Composable
fun AddTile(icon: ImageVector = FcIcons.Plus) {
    Box(
        Modifier.size(40.dp).clip(RoundedCornerShape(14.dp)).background(MaterialTheme.colorScheme.primaryContainer),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimaryContainer)
    }
}

/** The tab you're on shows its name, the others just their icons. */
@Composable
fun FloatingNav(
    current: Screen,
    frames: StateFlow<List<FrameStatus>>,
    busy: StateFlow<Boolean>,
    onTab: (Screen) -> Unit,
    modifier: Modifier = Modifier,
) {
    val list by frames.collectAsStateWithLifecycle()
    val syncing by busy.collectAsStateWithLifecycle()
    val frameDot = when {
        list.isEmpty() -> null
        list.any { it.state == FrameState.CONNECTED } -> LocalStatusColors.current.ok
        list.all { it.state == FrameState.CONNECTING } -> MaterialTheme.colorScheme.outline
        else -> MaterialTheme.colorScheme.error
    }
    HorizontalFloatingToolbar(
        expanded = true,
        modifier = modifier,
        colors = FloatingToolbarDefaults.standardFloatingToolbarColors(
            toolbarContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
        ),
    ) {
        NavItem(current is Screen.Library, FcIcons.Library, "library") { onTab(Screen.Library) }
        NavItem(current is Screen.Frame, FcIcons.Headset, "frame", badge = frameDot, pulsing = syncing) { onTab(Screen.Frame()) }
        NavItem(current is Screen.Settings, FcIcons.Settings, "settings") { onTab(Screen.Settings) }
    }
}

@Composable
private fun RowScope.NavItem(
    selected: Boolean,
    icon: ImageVector,
    label: String,
    badge: Color? = null,
    pulsing: Boolean = false,
    onClick: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    val container by animateColorAsState(if (selected) scheme.secondaryContainer else Color.Transparent, label = "tab")
    val tint by animateColorAsState(if (selected) scheme.onSecondaryContainer else scheme.onSurfaceVariant, label = "tab ink")
    Row(
        Modifier
            .padding(horizontal = 2.dp)
            .height(48.dp)
            .clip(CircleShape)
            .background(container)
            .selectable(selected = selected, role = Role.Tab, onClick = onClick)
            .animateContentSize(MaterialTheme.motionScheme.fastSpatialSpec())
            .padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box {
            Icon(icon, contentDescription = label, tint = tint)
            if (badge != null) {
                val ring by animateColorAsState(if (selected) scheme.secondaryContainer else scheme.surfaceContainerHigh, label = "dot ring")
                // on the headset's lower corner, overlapping it a little
                Box(Modifier.align(Alignment.BottomEnd).offset(4.dp, 1.dp), contentAlignment = Alignment.Center) {
                    if (pulsing) Pulse(badge)
                    StatusDot(badge, ring = ring)
                }
            }
        }
        AnimatedVisibility(selected, enter = fadeIn() + expandHorizontally(), exit = fadeOut() + shrinkHorizontally()) {
            Text(label, style = MaterialTheme.typography.labelLarge, color = tint, modifier = Modifier.padding(start = 8.dp, end = 4.dp))
        }
    }
}

/** A frame of the clip, made by the phone, or a play glyph until there is one. */
@Composable
fun ClipThumbnail(clip: Clip, gallery: MediaGallery, modifier: Modifier = Modifier) {
    val picture by produceState(remember(clip.location) { gallery.cachedThumbnail(clip.location)?.asImageBitmap() }, clip.location) {
        value = withContext(Dispatchers.IO) { gallery.thumbnail(clip.location)?.asImageBitmap() }
    }
    Box(modifier.background(MaterialTheme.colorScheme.surfaceContainerHighest), contentAlignment = Alignment.Center) {
        val p = picture
        if (p != null) {
            Image(p, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
        } else {
            Icon(FcIcons.Play, contentDescription = null, tint = MaterialTheme.colorScheme.outline, modifier = Modifier.size(28.dp))
        }
    }
}

/** Corners for a row in a segmented list: big at the ends of the group, small between. */
fun segmentShape(index: Int, count: Int, outer: Dp = 24.dp, inner: Dp = 4.dp): RoundedCornerShape {
    val top = if (index == 0) outer else inner
    val bottom = if (index == count - 1) outer else inner
    return RoundedCornerShape(topStart = top, topEnd = top, bottomStart = bottom, bottomEnd = bottom)
}

@Composable
fun SegmentRow(
    index: Int,
    count: Int,
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    titleColor: Color = MaterialTheme.colorScheme.onSurface,
    onClick: (() -> Unit)? = null,
    leading: (@Composable () -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 64.dp)
            .clip(segmentShape(index, count))
            .background(MaterialTheme.colorScheme.surfaceContainer)
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        leading?.invoke()
        Column(Modifier.weight(1f)) {
            Text(title, style = RowTitle, color = titleColor)
            if (subtitle != null) {
                Text(subtitle, style = MaterialTheme.typography.bodySmall.copy(lineHeight = 17.sp), color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        trailing?.invoke()
    }
}

private val RowTitle @Composable get() = MaterialTheme.typography.titleSmall.copy(fontSize = 15.sp, lineHeight = 21.sp)

@Composable
fun GroupLabel(text: String, modifier: Modifier = Modifier, color: Color = MaterialTheme.colorScheme.primary) {
    Text(
        text,
        style = MaterialTheme.typography.labelLarge,
        color = color,
        modifier = modifier.padding(start = 16.dp, end = 16.dp, top = 18.dp, bottom = 8.dp),
    )
}

@Composable
fun Chevron() {
    Icon(FcIcons.ChevronRight, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
}
