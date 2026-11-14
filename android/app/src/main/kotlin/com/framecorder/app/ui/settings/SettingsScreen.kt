package com.framecorder.app.ui.settings

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.app.ActivityCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.framecorder.app.APP_DOWNLOAD
import com.framecorder.app.AppUpdate
import com.framecorder.app.BuildConfig
import com.framecorder.app.Prefs
import com.framecorder.app.sync.SyncHub
import com.framecorder.app.sync.SyncService
import com.framecorder.app.ui.common.AddTile
import com.framecorder.app.ui.common.Chevron
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.FrameAvatar
import com.framecorder.app.ui.common.GroupLabel
import com.framecorder.app.ui.common.SegmentRow
import com.framecorder.app.ui.common.VIDEO_PERMISSION
import com.framecorder.app.ui.common.VIDEO_PERMISSION_WHY
import com.framecorder.app.ui.common.ago
import com.framecorder.app.ui.common.canNotify
import com.framecorder.app.ui.common.canSeeVideos
import com.framecorder.app.ui.common.dotted
import com.framecorder.app.ui.common.lastSync
import com.framecorder.app.ui.common.openDiscord
import com.framecorder.app.ui.common.stateText

@Composable
fun SettingsScreen(hub: SyncHub, prefs: Prefs, appUpdate: AppUpdate, onFrame: (String) -> Unit, onPair: () -> Unit, onReport: () -> Unit) {
    val context = LocalContext.current
    val frames by hub.frames.collectAsStateWithLifecycle()
    val clips by hub.clips.collectAsStateWithLifecycle()
    val wallpaper by prefs.wallpaper.collectAsStateWithLifecycle()
    val background by prefs.background.collectAsStateWithLifecycle()
    val clipNotes by prefs.clipNotes.collectAsStateWithLifecycle()
    val stats by prefs.stats.collectAsStateWithLifecycle()
    val newer by appUpdate.newer.collectAsStateWithLifecycle()
    var notifications by remember { mutableStateOf(context.canNotify()) }
    val askNotifications = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { notifications = it }
    var videos by remember { mutableStateOf(context.canSeeVideos()) }
    val askVideos = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        videos = context.canSeeVideos()
        // "don't ask again" means android won't show its dialog; its settings page still can
        if (!granted && !videos && !shouldExplain(context)) openAppSettings(context)
    }
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
            item(key = "app update") {
                AppUpdateCard(newer, onGet = { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(APP_DOWNLOAD))) })
            }

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
            val syncRows = if (videos) 2 else 3
            item {
                SegmentRow(
                    0, syncRows, "keep syncing in the background",
                    onClick = { toggleBackground(context, prefs, !background, frames.isNotEmpty()) },
                    trailing = { Toggle(background) { toggleBackground(context, prefs, it, frames.isNotEmpty()) } },
                )
            }
            item {
                SegmentRow(
                    1, syncRows, "clip notifications",
                    subtitle = if (notifications) null else "off in android",
                    onClick = if (!notifications) {
                        { askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS) }
                    } else {
                        { prefs.setClipNotes(!clipNotes) }
                    },
                    trailing = if (!notifications) {
                        { Text("turn on", color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelLarge) }
                    } else {
                        { Toggle(clipNotes, prefs::setClipNotes) }
                    },
                )
            }
            // once it's allowed there's nothing left to do here
            if (!videos) item {
                SegmentRow(
                    2, syncRows, "see videos saved before",
                    subtitle = VIDEO_PERMISSION_WHY,
                    onClick = { askVideos.launch(VIDEO_PERMISSION) },
                    trailing = { Text("turn on", color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelLarge) },
                )
            }

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                item { GroupLabel("look") }
                item {
                    SegmentRow(
                        0, 1, "use wallpaper colors",
                        onClick = { prefs.setWallpaper(!wallpaper) },
                        trailing = { Toggle(wallpaper, prefs::setWallpaper) },
                    )
                }
            }

            item { GroupLabel("this phone") }
            item {
                SegmentRow(
                    0, 5, "saved to",
                    subtitle = "Movies/framecorder",
                    leading = { Icon(FcIcons.Folder, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant) },
                )
            }
            item {
                SegmentRow(
                    1, 5, "report an issue",
                    onClick = onReport,
                    leading = { Icon(FcIcons.Bug, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant) },
                    trailing = { Chevron() },
                )
            }
            item {
                SegmentRow(
                    2, 5, "join the discord",
                    onClick = { context.openDiscord() },
                    leading = { Icon(FcIcons.Discord, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant) },
                    trailing = { Chevron() },
                )
            }
            item {
                SegmentRow(
                    3, 5, "anonymous stats",
                    subtitle = "once a day: this app's version and system. nothing about you or your videos",
                    onClick = { prefs.setStats(!stats) },
                    trailing = { Toggle(stats, prefs::setStats) },
                )
            }
            item {
                SegmentRow(4, 5, "framecorder ${BuildConfig.VERSION_NAME}", subtitle = "fonts: montserrat, poppins, space grotesk (sil ofl)")
            }
            item { Spacer(Modifier.height(8.dp)) }
        }
    }
}

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

private fun toggleBackground(context: Context, prefs: Prefs, on: Boolean, paired: Boolean) {
    prefs.setBackground(on)
    if (on && paired) SyncService.start(context) else SyncService.stop(context)
}

/** Whether android would still show its own dialog for the videos permission. */
private fun shouldExplain(context: Context): Boolean =
    (context as? Activity)?.let { ActivityCompat.shouldShowRequestPermissionRationale(it, VIDEO_PERMISSION) } ?: true

private fun openAppSettings(context: Context) {
    val intent = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null))
    runCatching { context.startActivity(intent) }
}
