package com.framecorder.app.ui.library

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.lerp
import androidx.compose.ui.unit.sp
import com.framecorder.app.sync.MediaGallery
import com.framecorder.app.ui.common.ClipThumbnail
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.StatusDot
import com.framecorder.app.ui.common.day
import com.framecorder.app.ui.common.length
import com.framecorder.app.ui.common.sharedClip
import com.framecorder.app.ui.common.time
import com.framecorder.app.ui.theme.DataStyle
import com.framecorder.app.ui.theme.LocalStatusColors
import com.framecorder.core.Clip

/**
 * A clip as a square of its picture, with its length up in the corner the way a photos app marks videos.
 * A long press selects it; while anything's selected, a tap selects or deselects instead of playing, and the
 * selected ones shrink into their tile with a check on them.
 */
@Composable
fun ClipTile(
    clip: Clip,
    gallery: MediaGallery,
    shape: RoundedCornerShape,
    /** Whether any clip is selected, so a tap selects instead of playing. */
    selecting: Boolean,
    selected: Boolean,
    onOpen: () -> Unit,
    onToggle: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val recording = clip.kind != "clip"
    val said = listOfNotNull(if (recording) "recording" else "clip", "${day(clip.created)}, ${time(clip.created)}", clip.durationS?.let(::length))
    val haptics = LocalHapticFeedback.current
    // how far into its selected look the tile is
    val picked by animateFloatAsState(if (selected) 1f else 0f, MaterialTheme.motionScheme.fastSpatialSpec(), label = "picked")
    Box(
        modifier
            .aspectRatio(1f)
            // the whole tile goes to the player and back, corners and all, so nothing pops in after
            .sharedClip(clip.key, shape)
            .combinedClickable(
                onClickLabel = when {
                    !selecting -> "play"
                    selected -> "deselect"
                    else -> "select"
                },
                onClick = if (selecting) onToggle else onOpen,
                onLongClickLabel = if (selected) "deselect" else "select",
                onLongClick = {
                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                    onToggle()
                },
            )
            .semantics {
                contentDescription = said.joinToString(", ")
                if (selecting) this.selected = selected
            },
    ) {
        Box(
            Modifier.fillMaxSize().graphicsLayer {
                scaleX = 1f - picked * 0.14f
                scaleY = scaleX
                this.shape = RoundedCornerShape(lerp(3.dp, 14.dp, picked))
                this.clip = true
            },
        ) {
            ClipThumbnail(clip, gallery, Modifier.fillMaxSize())
            // just enough shade up top for the length to read on a bright picture
            Box(
                Modifier
                    .align(Alignment.TopCenter)
                    .fillMaxWidth()
                    .height(36.dp)
                    .background(Brush.verticalGradient(listOf(Color.Black.copy(alpha = 0.35f), Color.Transparent))),
            )
            Row(
                Modifier.align(Alignment.TopEnd).padding(horizontal = 8.dp, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                if (recording) StatusDot(LocalStatusColors.current.rec, size = 7.dp)
                clip.durationS?.let { Text(length(it), style = TileLength) }
                Icon(FcIcons.Play, contentDescription = null, tint = Color.White, modifier = Modifier.size(12.dp))
            }
        }
        AnimatedVisibility(
            visible = selecting,
            enter = fadeIn() + scaleIn(MaterialTheme.motionScheme.fastSpatialSpec(), initialScale = 0.6f),
            exit = fadeOut() + scaleOut(targetScale = 0.6f),
            modifier = Modifier.align(Alignment.TopStart).padding(6.dp),
        ) {
            PickMark(selected)
        }
    }
}

/** The circle in a tile's corner while selecting: hollow until the tile's selected, then filled with a check. */
@Composable
private fun PickMark(selected: Boolean) {
    val scheme = MaterialTheme.colorScheme
    Box(
        Modifier
            .size(22.dp)
            .clip(CircleShape)
            .background(if (selected) scheme.primary else Color.Black.copy(alpha = 0.25f))
            .border(1.5.dp, if (selected) scheme.primary else Color.White.copy(alpha = 0.9f), CircleShape),
        contentAlignment = Alignment.Center,
    ) {
        if (selected) Icon(FcIcons.Check, contentDescription = null, tint = scheme.onPrimary, modifier = Modifier.size(14.dp))
    }
}

/** Big corners where a day's block of tiles has its outside corners, small ones between tiles, so each day reads as one shape. */
fun tileShape(i: Int, n: Int, columns: Int): RoundedCornerShape {
    val row = i / columns
    val col = i % columns
    val lastRow = (n - 1) / columns
    val rowEnd = if (row == lastRow) (n - 1) % columns else columns - 1
    // a short last row leaves the corner above its end showing too
    val stepped = row == lastRow - 1 && col == columns - 1 && (n - 1) % columns != columns - 1
    fun corner(outside: Boolean) = if (outside) 18.dp else 3.dp
    return RoundedCornerShape(
        topStart = corner(row == 0 && col == 0),
        topEnd = corner(row == 0 && col == rowEnd),
        bottomStart = corner(row == lastRow && col == 0),
        bottomEnd = corner((row == lastRow && col == rowEnd) || stepped),
    )
}

private val TileLength = DataStyle.copy(
    fontSize = 12.sp,
    lineHeight = 14.sp,
    color = Color.White,
    shadow = Shadow(Color.Black.copy(alpha = 0.6f), blurRadius = 6f),
)
