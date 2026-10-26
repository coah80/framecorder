package com.framecorder.app.ui.settings

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import com.framecorder.app.BuildConfig
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.ShapeIcon

/**
 * A newer framecorder for this phone, quietly at the top of settings. It's
 * the app's own update, not the headset's (that one's on the frame tab, and
 * the headset installs it itself); this one opens the download in the browser.
 */
@Composable
fun AppUpdateCard(newer: String?, onGet: () -> Unit, modifier: Modifier = Modifier) {
    AnimatedVisibility(
        visible = newer != null,
        enter = fadeIn() + expandVertically(MaterialTheme.motionScheme.defaultSpatialSpec()),
        exit = fadeOut() + shrinkVertically(),
        modifier = modifier,
    ) {
        val scheme = MaterialTheme.colorScheme
        Row(
            Modifier
                .fillMaxWidth()
                .padding(top = 4.dp)
                .clip(RoundedCornerShape(24.dp))
                .background(scheme.surfaceContainerHigh)
                .padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            ShapeIcon(
                FcIcons.Phone,
                shape = MaterialShapes.Cookie9Sided,
                size = 40.dp,
                iconSize = 20.dp,
                container = scheme.secondaryContainer,
                content = scheme.onSecondaryContainer,
            )
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text("framecorder ${newer ?: ""} is out", style = MaterialTheme.typography.titleSmall)
                Text(
                    "this phone has ${BuildConfig.VERSION_NAME}. it downloads in your browser, then android asks to install it.",
                    style = MaterialTheme.typography.bodySmall,
                    color = scheme.onSurfaceVariant,
                )
            }
            FilledTonalButton(onClick = onGet, shapes = ButtonDefaults.shapes()) { Text("get it") }
        }
    }
}
