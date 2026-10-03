package com.framecorder.app.ui.common

import androidx.compose.animation.EnterExitState
import androidx.compose.animation.SharedTransitionScope
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.spring
import androidx.compose.foundation.shape.CornerBasedShape
import androidx.compose.foundation.shape.CornerSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.dp
import androidx.navigation3.ui.LocalNavAnimatedContentScope
import com.framecorder.app.ui.LocalSharedScope

/** The player's corners. A tile's own corners turn into these on the way there and back. */
val PlayerCorner = 24.dp

private fun <T> grow() = spring<T>(dampingRatio = 0.82f, stiffness = 380f)

/**
 * Marks a clip's picture, so opening it grows the thumbnail into the player and back.
 * Everything after it in the chain goes along, clipped to [shape] on its own page and
 * easing into the player's corners on the way.
 */
@Composable
fun Modifier.sharedClip(key: String, shape: CornerBasedShape = RoundedCornerShape(PlayerCorner)): Modifier {
    val shared = LocalSharedScope.current ?: return clip(shape)
    val visibility = LocalNavAnimatedContentScope.current
    val state = shared.rememberSharedContentState("clip-$key")
    val settled = visibility.transition.animateFloat({ grow() }, label = "corners") { if (it == EnterExitState.Visible) 1f else 0f }
    val own = shape
    return with(shared) {
        this@sharedClip
            .sharedBounds(
                sharedContentState = state,
                animatedVisibilityScope = visibility,
                resizeMode = SharedTransitionScope.ResizeMode.RemeasureToBounds,
                boundsTransform = { _, _ -> grow() },
            )
            .graphicsLayer {
                // only on the way to or from the player, not while a whole page fades for a tab
                val f = if (state.isMatchFound) settled.value.coerceIn(0f, 1f) else 1f
                val player = PlayerCorner.toPx()
                fun corner(c: CornerSize) = CornerSize(player + (c.toPx(size, this) - player) * f)
                this.shape = RoundedCornerShape(
                    topStart = corner(own.topStart),
                    topEnd = corner(own.topEnd),
                    bottomEnd = corner(own.bottomEnd),
                    bottomStart = corner(own.bottomStart),
                )
                clip = true
            }
    }
}
