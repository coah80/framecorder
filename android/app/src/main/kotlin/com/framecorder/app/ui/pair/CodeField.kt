package com.framecorder.app.ui.pair

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.keyframes
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.framecorder.app.ui.theme.SpaceGrotesk
import kotlinx.coroutines.delay

/**
 * The 6-digit code as six boxes. Digits are tabular and the boxes fixed
 * width, so nothing shifts as they arrive; each one rolls up into its box,
 * and a full code lights them all up and waves through them, left to right.
 */
@Composable
fun CodeField(
    code: String,
    onChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    focus: FocusRequester = FocusRequester(),
) {
    BasicTextField(
        value = code,
        onValueChange = { onChange(it.filter(Char::isDigit).take(6)) },
        enabled = enabled,
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword, imeAction = ImeAction.Done),
        modifier = modifier.focusRequester(focus).semantics { contentDescription = "pairing code from your frame" },
        decorationBox = { _ ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                for (i in 0 until 6) {
                    if (i == 3) Spacer(Modifier.width(6.dp))
                    DigitBox(digit = code.getOrNull(i), active = enabled && i == code.length, done = code.length == 6, index = i)
                }
            }
        },
    )
}

@Composable
private fun DigitBox(digit: Char?, active: Boolean, done: Boolean, index: Int) {
    val scheme = MaterialTheme.colorScheme
    val stagger = tween<androidx.compose.ui.graphics.Color>(durationMillis = 220, delayMillis = if (done) index * 45 else 0)
    val fill by animateColorAsState(
        when {
            done -> scheme.primaryContainer
            digit != null -> scheme.surfaceContainerHigh
            else -> scheme.surfaceContainer
        },
        stagger,
        label = "box",
    )
    val edge by animateColorAsState(
        when {
            done || active -> scheme.primary
            digit != null -> scheme.outline
            else -> scheme.outlineVariant
        },
        stagger,
        label = "edge",
    )
    val grow by animateFloatAsState(if (active) 1.06f else 1f, MaterialTheme.motionScheme.fastSpatialSpec(), label = "active")
    val roll = MaterialTheme.motionScheme.fastSpatialSpec<IntOffset>()
    // a full code sends a little wave through the boxes, left to right
    val wave = remember { Animatable(0f) }
    val settle = MaterialTheme.motionScheme.defaultSpatialSpec<Float>()
    LaunchedEffect(done) {
        if (!done) return@LaunchedEffect
        delay(index * 55L)
        wave.animateTo(-8f, tween(170))
        wave.animateTo(0f, settle)
    }
    Box(
        Modifier
            .size(width = 46.dp, height = 58.dp)
            .graphicsLayer { translationY = wave.value.dp.toPx() }
            .scale(grow)
            .clip(RoundedCornerShape(14.dp))
            .background(fill)
            .border(if (active || done) 2.dp else 1.dp, edge, RoundedCornerShape(14.dp)),
        contentAlignment = Alignment.Center,
    ) {
        AnimatedContent(
            targetState = digit,
            transitionSpec = {
                (slideInVertically(roll) { it } + scaleIn(initialScale = 0.6f) + fadeIn()) togetherWith fadeOut(tween(90))
            },
            label = "digit",
        ) { d ->
            if (d != null) {
                Text(
                    d.toString(),
                    style = MaterialTheme.typography.headlineMedium.copy(
                        fontFamily = SpaceGrotesk,
                        fontSize = 28.sp,
                        fontFeatureSettings = "tnum",
                    ),
                    color = if (done) scheme.onPrimaryContainer else scheme.onSurface,
                )
            } else if (active) {
                val blink by rememberInfiniteTransition(label = "caret").animateFloat(
                    1f,
                    0f,
                    infiniteRepeatable(keyframes {
                        durationMillis = 1000
                        1f at 499
                        0f at 500
                    }),
                    label = "caret",
                )
                Box(Modifier.width(2.dp).height(28.dp).graphicsLayer { alpha = blink }.background(scheme.primary))
            }
        }
    }
}
