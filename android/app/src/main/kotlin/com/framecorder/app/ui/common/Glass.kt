package com.framecorder.app.ui.common

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import dev.chrisbanes.haze.HazeInput
import dev.chrisbanes.haze.HazeProgressive
import dev.chrisbanes.haze.HazeState
import dev.chrisbanes.haze.blur.HazeBlurStyle
import dev.chrisbanes.haze.blur.HazeColorEffect
import dev.chrisbanes.haze.blur.hazeBlur

/**
 * Covers the top of a scrolling page: what goes under the status bar blurs
 * more the higher it gets, with just enough tint to keep the icons readable.
 * [shown] fades it in as the page starts going under it, so nothing's
 * blurred while the page sits at the top.
 */
@Composable
fun StatusBarBlur(state: HazeState, height: Dp, shown: () -> Float, modifier: Modifier = Modifier) {
    val surface = MaterialTheme.colorScheme.surface
    val tint = Brush.verticalGradient(0f to surface.copy(alpha = 0.8f), 1f to surface.copy(alpha = 0f))
    Box(
        modifier.fillMaxWidth().height(height).graphicsLayer { alpha = shown() }.hazeBlur(
            input = HazeInput.Sources(state),
            style = HazeBlurStyle {
                blurRadius(24.dp)
                noiseFactor(0f)
                // lists don't draw the page under their items, so without it the gaps blur see-through
                backgroundColor(surface)
                progressive(HazeProgressive.verticalGradient(startIntensity = 1f, endIntensity = 0f))
                colorEffects(listOf(HazeColorEffect.tint(tint)))
                fallbackColorEffect(HazeColorEffect.tint(tint))
            },
        ),
    )
}

/** Frosted glass over the page, for things that float on it. */
@Composable
fun Modifier.glass(state: HazeState): Modifier {
    val container = MaterialTheme.colorScheme.surfaceContainerHigh
    val page = MaterialTheme.colorScheme.surface
    return hazeBlur(
        input = HazeInput.Sources(state),
        style = HazeBlurStyle {
            blurRadius(28.dp)
            noiseFactor(0f)
            backgroundColor(page)
            colorEffects(listOf(HazeColorEffect.tint(container.copy(alpha = 0.72f))))
            fallbackColorEffect(HazeColorEffect.tint(container.copy(alpha = 0.96f)))
        },
    )
}

/** A pill of frosted glass that floats centered at the top of a scrolled page. */
@Composable
fun GlassPill(
    haze: HazeState,
    onClick: () -> Unit,
    onClickLabel: String,
    modifier: Modifier = Modifier,
    padding: PaddingValues = PaddingValues(horizontal = 22.dp),
    content: @Composable RowScope.() -> Unit,
) {
    Row(
        modifier
            .shadow(10.dp, CircleShape)
            .clip(CircleShape)
            .glass(haze)
            .border(1.dp, Color.White.copy(alpha = 0.07f), CircleShape)
            .clickable(role = Role.Button, onClickLabel = onClickLabel, onClick = onClick)
            .height(60.dp)
            .padding(padding)
            .animateContentSize(MaterialTheme.motionScheme.fastSpatialSpec()),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        content = content,
    )
}

/** The thin divider between things in a pill. */
@Composable
fun PillRule() {
    Box(Modifier.width(1.dp).height(20.dp).background(MaterialTheme.colorScheme.outlineVariant))
}
