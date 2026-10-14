package com.framecorder.app.ui.frame

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.ShapeIcon
import com.framecorder.app.ui.theme.DataStyle
import com.framecorder.core.FrameUpdate

/**
 * A framecorder update for the headset, at the top of the frame tab: what's
 * out and a button to install it now, then the wave while the headset does
 * it. Gone once the frame says it's done (or when it has nothing to say).
 * [update] is what the frame last said; [updating] is also true while we've
 * asked and it hasn't confirmed yet.
 */
@Composable
fun FrameUpdateCard(update: FrameUpdate?, updating: Boolean, onUpdate: () -> Unit, modifier: Modifier = Modifier) {
    val offer = update?.takeIf { it.available && !updating }
    AnimatedVisibility(
        visible = offer != null || updating,
        enter = fadeIn() + expandVertically(MaterialTheme.motionScheme.defaultSpatialSpec()),
        exit = fadeOut() + shrinkVertically(),
        modifier = modifier,
    ) {
        val scheme = MaterialTheme.colorScheme
        val container by animateColorAsState(if (updating) scheme.surfaceContainerHigh else scheme.primaryContainer, label = "update")
        val ink by animateColorAsState(if (updating) scheme.onSurface else scheme.onPrimaryContainer, label = "update ink")
        val soft by animateColorAsState(if (updating) scheme.onSurfaceVariant else scheme.onPrimaryContainer.copy(alpha = 0.78f), label = "update note")
        Column(
            Modifier
                .fillMaxWidth()
                .padding(bottom = 12.dp)
                .clip(RoundedCornerShape(24.dp))
                .background(container)
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                ShapeIcon(
                    FcIcons.Down,
                    shape = MaterialShapes.Clover8Leaf,
                    size = 40.dp,
                    iconSize = 20.dp,
                    container = scheme.primary,
                    content = scheme.onPrimary,
                )
                Column(Modifier.weight(1f)) {
                    Text("framecorder on the headset", style = MaterialTheme.typography.labelMedium, color = soft)
                    Text(
                        when {
                            updating -> "updating"
                            offer?.latest != null -> "${offer.latest} is out"
                            else -> "a newer version is out"
                        },
                        style = MaterialTheme.typography.titleMedium,
                        color = ink,
                    )
                }
                update?.installed?.takeIf { it.isNotBlank() }?.let { installed ->
                    Text(
                        "has $installed",
                        style = DataStyle.copy(color = ink),
                        modifier = Modifier.clip(CircleShape).background(ink.copy(alpha = 0.12f)).padding(horizontal = 12.dp, vertical = 6.dp),
                    )
                }
            }
            AnimatedContent(updating, transitionSpec = { fadeIn() togetherWith fadeOut() }, label = "update body") { busy ->
                if (busy) {
                    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        LinearWavyProgressIndicator(
                            modifier = Modifier.fillMaxWidth(),
                            color = scheme.primary,
                            trackColor = scheme.onSurface.copy(alpha = 0.12f),
                        )
                        Text(
                            "the headset is installing it. framecorder restarts there on its own, so it drops off here for a moment and comes back on the new version.",
                            style = MaterialTheme.typography.bodySmall,
                            color = soft,
                        )
                    }
                } else {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Text(
                            "it installs itself within a few hours anyway. this does it now, while nobody's wearing the headset.",
                            style = MaterialTheme.typography.bodySmall,
                            color = soft,
                            modifier = Modifier.weight(1f),
                        )
                        Button(
                            onClick = onUpdate,
                            shapes = ButtonDefaults.shapes(),
                            colors = ButtonDefaults.buttonColors(containerColor = scheme.primary, contentColor = scheme.onPrimary),
                        ) { Text("update now") }
                    }
                }
            }
        }
    }
}
