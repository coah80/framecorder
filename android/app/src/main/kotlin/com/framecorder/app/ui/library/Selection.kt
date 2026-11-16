package com.framecorder.app.ui.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.GlassPill
import com.framecorder.app.ui.common.size
import com.framecorder.core.Clip
import dev.chrisbanes.haze.HazeState

/** "3 selected · 48 MB" */
fun selectedText(clips: List<Clip>) = "${clips.size} selected · ${size(clips.sumOf { it.size })}"

/** In the header's row while selecting: a close and the count, where search and more were. */
@Composable
fun SelectionTitle(picked: List<Clip>, onClear: () -> Unit, modifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth().height(56.dp), verticalAlignment = Alignment.CenterVertically) {
        IconButton(onClick = onClear) { Icon(FcIcons.Close, contentDescription = "stop selecting") }
        Text(selectedText(picked), style = MaterialTheme.typography.titleMedium, maxLines = 1)
    }
}

/** The day pill's place once the page has scrolled: the count, and tapping it stops selecting. */
@Composable
fun SelectionPill(picked: List<Clip>, haze: HazeState, onClear: () -> Unit) {
    GlassPill(haze, onClick = onClear, onClickLabel = "stop selecting") {
        Icon(FcIcons.Close, contentDescription = null, modifier = Modifier.size(20.dp))
        Text(selectedText(picked), style = MaterialTheme.typography.titleMedium.copy(fontSize = 17.sp, fontWeight = FontWeight.SemiBold), maxLines = 1)
    }
}

/** What to do with the selected clips, laid out like the player's own row: a pill of actions and share as the big one. */
@Composable
fun SelectionBar(picked: List<Clip>, allPicked: Boolean, onSelectAll: () -> Unit, onDelete: () -> Unit, onShare: () -> Unit, modifier: Modifier = Modifier) {
    Row(
        modifier
            .navigationBarsPadding()
            .padding(start = 16.dp, end = 16.dp, bottom = 24.dp)
            .fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Surface(
            shape = CircleShape,
            color = MaterialTheme.colorScheme.surfaceContainerHigh,
            shadowElevation = 6.dp,
            modifier = Modifier.weight(1f).height(64.dp),
        ) {
            Row(Modifier.padding(horizontal = 8.dp), horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onSelectAll) {
                    Icon(FcIcons.SelectAll, contentDescription = if (allPicked) "select none" else "select all")
                }
                IconButton(onClick = onDelete) { Icon(FcIcons.Delete, contentDescription = "delete from this phone") }
            }
        }
        ExtendedFloatingActionButton(
            onClick = onShare,
            icon = { Icon(FcIcons.Share, contentDescription = null) },
            text = { Text("share", style = MaterialTheme.typography.titleMedium) },
            containerColor = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            shape = RoundedCornerShape(20.dp),
            modifier = Modifier.height(64.dp),
        )
    }
}
