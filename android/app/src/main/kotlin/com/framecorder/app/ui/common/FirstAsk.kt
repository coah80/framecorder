package com.framecorder.app.ui.common

import android.Manifest
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp

/**
 * The first time the app opens: what it's about to ask android for, in a
 * sentence each, before the system dialogs. [wants] is which permissions
 * aren't granted yet (see [firstAsk]); the videos one matters most, since
 * without it a reinstall downloads every clip again.
 */
@Composable
fun FirstAsk(wants: List<String>, onDismiss: () -> Unit, onContinue: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = MaterialTheme.colorScheme.surfaceContainerLow) {
        Column(
            Modifier.navigationBarsPadding().padding(start = 24.dp, end = 24.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(dotted("permissions"), style = MaterialTheme.typography.headlineSmall)
            if (VIDEO_PERMISSION in wants) {
                Line(FcIcons.Library, "videos", VIDEO_PERMISSION_WHY)
            }
            if (Manifest.permission.POST_NOTIFICATIONS in wants) {
                Line(FcIcons.Bell, "notifications", "when a clip arrives")
            }
            Button(onClick = onContinue, shapes = ButtonDefaults.shapes(), modifier = Modifier.fillMaxWidth().padding(top = 4.dp)) { Text("ok") }
        }
    }
}

@Composable
private fun Line(icon: ImageVector, title: String, why: String) {
    Row(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.Top) {
        Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(top = 2.dp))
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, style = MaterialTheme.typography.titleSmall)
            Text(why, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}
