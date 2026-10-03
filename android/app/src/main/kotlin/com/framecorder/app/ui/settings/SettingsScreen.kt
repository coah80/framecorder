package com.framecorder.app.ui.settings

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.Icon
import androidx.compose.material3.LargeFlexibleTopAppBar
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.framecorder.app.BuildConfig
import com.framecorder.app.Prefs
import com.framecorder.app.sync.SyncHub
import com.framecorder.app.sync.SyncService
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.GroupLabel
import com.framecorder.app.ui.common.SegmentRow
import com.framecorder.app.ui.common.AddTile
import com.framecorder.app.ui.common.Chevron
import com.framecorder.app.ui.common.FrameAvatar
import com.framecorder.app.ui.common.dotted
import com.framecorder.app.ui.common.ago
import com.framecorder.app.ui.common.lastSync
import com.framecorder.app.ui.common.stateText
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.lazy.itemsIndexed

@Composable
fun SettingsScreen(hub: SyncHub, prefs: Prefs, onFrame: (String) -> Unit, onPair: () -> Unit) {
    val context = LocalContext.current
    val frames by hub.frames.collectAsStateWithLifecycle()
    val clips by hub.clips.collectAsStateWithLifecycle()
    val wallpaper by prefs.wallpaper.collectAsStateWithLifecycle()
    val background by prefs.background.collectAsStateWithLifecycle()
    var notifications by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED,
        )
    }
    val askNotifications = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { notifications = it }
    var howOpen by rememberSaveable { mutableStateOf(false) }
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()

    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        containerColor = MaterialTheme.colorScheme.surface,
        contentWindowInsets = WindowInsets(0),
        topBar = {
            LargeFlexibleTopAppBar(
                title = { Text(dotted("settings")) },
                scrollBehavior = scroll,
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                    scrolledContainerColor = MaterialTheme.colorScheme.surfaceContainer,
                ),
            )
        },
    ) { padding ->
        LazyColumn(
            Modifier.fillMaxSize().padding(top = padding.calculateTopPadding()),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 140.dp),
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            item { GroupLabel("frames") }
            val count = frames.size + 1
            itemsIndexed(frames, key = { _, f -> f.fingerprint }) { i, f ->
                SegmentRow(
                    i, count, f.name,
                    subtitle = listOfNotNull(stateText(f), lastSync(clips, f.fingerprint)?.let { "synced ${ago(it)}" }).joinToString(" · "),
                    onClick = { onFrame(f.fingerprint) },
                    leading = { FrameAvatar(f.state) },
                    trailing = { Chevron() },
                )
            }
            item {
                SegmentRow(
                    count - 1, count, "pair a frame",
                    titleColor = MaterialTheme.colorScheme.primary,
                    onClick = onPair,
                    leading = { AddTile() },
                )
            }

            item { GroupLabel("syncing") }
            item {
                SegmentRow(
                    0, 2, "keep syncing in the background",
                    subtitle = if (background) "android shows a quiet notification while this is on" else "clips sync while the app is open",
                    onClick = { toggleBackground(context, prefs, !background, frames.isNotEmpty()) },
                    trailing = { Toggle(background) { toggleBackground(context, prefs, it, frames.isNotEmpty()) } },
                )
            }
            item {
                SegmentRow(
                    1, 2, "notifications",
                    subtitle = if (notifications) "one per clip that lands, with share" else "off, so new clips arrive quietly",
                    onClick = if (!notifications && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                        { askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS) }
                    } else {
                        null
                    },
                    trailing = if (!notifications) {
                        { Text("turn on", color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelLarge) }
                    } else {
                        null
                    },
                )
            }

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                item { GroupLabel("look") }
                item {
                    SegmentRow(
                        0, 1, "use wallpaper colors",
                        subtitle = "matches your phone instead of framecorder purple",
                        onClick = { prefs.setWallpaper(!wallpaper) },
                        trailing = { Toggle(wallpaper, prefs::setWallpaper) },
                    )
                }
            }

            item { GroupLabel("this phone") }
            item {
                SegmentRow(
                    0, 3, "saved to",
                    subtitle = "Movies/framecorder, clips in /clips. your gallery app shows them too",
                    leading = { Icon(FcIcons.Folder, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant) },
                )
            }
            item {
                SegmentRow(
                    1, 3, "how syncing works",
                    subtitle = if (howOpen) null else "when and how clips get here",
                    onClick = { howOpen = !howOpen },
                    leading = { Icon(FcIcons.Info, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant) },
                )
            }
            if (howOpen) {
                item {
                    Column(
                        Modifier.padding(horizontal = 20.dp, vertical = 12.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        for (line in HOW) {
                            Text("· $line", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
            item {
                SegmentRow(2, 3, "framecorder ${BuildConfig.VERSION_NAME}", subtitle = "fonts: montserrat, poppins, space grotesk (sil ofl)")
            }
            item { Spacer(Modifier.height(8.dp)) }
        }
    }
}

private val HOW = listOf(
    "a clip or recording comes over the moment it's saved on your frame.",
    "only while the frame is on, on the same wi-fi as this phone, with framecorder running on it.",
    "if this app is closed or android stops it, anything new catches up the next time it's open.",
    "nothing goes through the internet. it's straight from the frame to this phone, encrypted, and only to the frame you paired.",
)

@Composable
private fun Toggle(checked: Boolean, onChange: (Boolean) -> Unit) {
    Switch(
        checked = checked,
        onCheckedChange = onChange,
        thumbContent = if (checked) {
            { Icon(FcIcons.Check, contentDescription = null, modifier = Modifier.size(SwitchDefaults.IconSize)) }
        } else {
            null
        },
    )
}

private fun toggleBackground(context: android.content.Context, prefs: Prefs, on: Boolean, paired: Boolean) {
    prefs.setBackground(on)
    if (on && paired) SyncService.start(context) else SyncService.stop(context)
}
