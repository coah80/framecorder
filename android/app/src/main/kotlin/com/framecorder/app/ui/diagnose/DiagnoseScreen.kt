package com.framecorder.app.ui.diagnose

import android.content.Context
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Build
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.framecorder.app.sync.SyncHub
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.ShapeIcon
import com.framecorder.app.ui.common.ago
import com.framecorder.app.ui.common.host
import com.framecorder.app.ui.common.lastSync
import com.framecorder.app.ui.common.segmentShape
import com.framecorder.app.ui.theme.LocalStatusColors
import com.framecorder.core.FrameState
import com.framecorder.core.sweepNetworks
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull

private enum class Result { Waiting, Running, Pass, Fail, Skipped }

private data class Check(val title: String, val detail: String, val result: Result)

@Composable
fun DiagnoseScreen(hub: SyncHub, fingerprint: String, onBack: () -> Unit, onPair: () -> Unit) {
    val context = LocalContext.current
    val frames by hub.frames.collectAsStateWithLifecycle()
    val clips by hub.clips.collectAsStateWithLifecycle()
    val synced = lastSync(clips, fingerprint)?.let { "last synced ${ago(it)}. " } ?: ""
    val frame = frames.firstOrNull { it.fingerprint == fingerprint }
    val name = frame?.name ?: "your frame"
    val addr = frame?.addr?.let(::host) ?: "its last address"
    val checks = remember { mutableStateListOf<Check>() }
    var run by remember { mutableIntStateOf(0) }

    LaunchedEffect(run) {
        checks.clear()
        fun set(i: Int, c: Check) {
            if (i < checks.size) checks[i] = c else checks.add(c)
        }

        val wifi = onWifi(context)
        set(0, Check("this phone is on wi-fi", if (wifi) sweepNetworks().joinToString().ifEmpty { "connected" } else "it isn't. the frame is only reachable on the same wi-fi.", if (wifi) Result.Pass else Result.Fail))

        // Android 17 asks before an app can talk to the local network
        val allowed = Build.VERSION.SDK_INT < 37 ||
            ContextCompat.checkSelfPermission(context, "android.permission.ACCESS_LOCAL_NETWORK") == PackageManager.PERMISSION_GRANTED
        set(1, Check("allowed to see devices on wi-fi", if (allowed) "android's local network permission" else "not allowed. pair again to be asked.", if (allowed) Result.Pass else Result.Fail))

        set(2, Check("answers at $addr", "asking...", Result.Running))
        val problem = hub.check(fingerprint)
        set(2, Check("answers at $addr", problem ?: "it's there and it's the frame you paired", if (problem == null) Result.Pass else Result.Fail))

        // only worth looking for if it isn't where it was
        set(3, Check("found by name", "asking the wi-fi...", if (problem == null) Result.Skipped else Result.Running))
        set(4, Check("found by asking every address", "checking ${sweepNetworks().joinToString().ifEmpty { "this network" }}...", if (problem == null) Result.Skipped else Result.Running))
        if (problem != null) {
            val byName = withTimeoutOrNull(6_000) { hub.finder.browse().first { it.fingerprint == fingerprint } }
            set(3, Check("found by name", byName?.let { "at ${host(it.addr)}" } ?: "no answer. some routers block this, the next check doesn't need it.", if (byName != null) Result.Pass else Result.Fail))
            val direct = hub.sweep(8).firstOrNull { it.fingerprint == fingerprint }
            set(4, Check("found by asking every address", direct?.let { "at ${host(it.addr)}" } ?: "no frame answered on this network", if (direct != null) Result.Pass else Result.Fail))
            if (byName != null || direct != null) hub.retryNow()
        } else {
            set(3, Check("found by name", "no need, it answered", Result.Skipped))
            set(4, Check("found by asking every address", "no need, it answered", Result.Skipped))
        }
    }

    val connected = frame?.state == FrameState.CONNECTED
    val trouble = !connected && checks.any { it.result == Result.Fail }
    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface)) {
        Column(
            Modifier.fillMaxSize().statusBarsPadding().verticalScroll(rememberScrollState()).padding(bottom = 140.dp),
        ) {
            Row(Modifier.height(64.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onBack) { Icon(FcIcons.Back, contentDescription = "back") }
                Text("connection check", style = MaterialTheme.typography.titleLarge.copy(fontFamily = MaterialTheme.typography.bodyLarge.fontFamily))
            }
            Column(Modifier.padding(horizontal = 24.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                ShapeIcon(
                    if (connected) FcIcons.Wifi else FcIcons.WifiOff,
                    shape = MaterialShapes.Cookie9Sided,
                    size = 72.dp,
                    iconSize = 30.dp,
                    container = if (connected) LocalStatusColors.current.okContainer else MaterialTheme.colorScheme.errorContainer,
                    content = if (connected) LocalStatusColors.current.ok else MaterialTheme.colorScheme.onErrorContainer,
                )
                Text(
                    if (connected) "$name is connected" else "can't reach $name",
                    style = MaterialTheme.typography.headlineMedium,
                )
                Text(
                    if (connected) "${synced}new clips come over the moment they're saved."
                    else "${synced}anything it saves waits on the frame, nothing is lost.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Column(Modifier.padding(horizontal = 16.dp).padding(top = 8.dp).animateContentSize(), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                checks.forEachIndexed { i, c -> CheckRow(c, i, checks.size) }
            }
            if (trouble) {
                Row(
                    Modifier
                        .padding(16.dp)
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(20.dp))
                        .background(MaterialTheme.colorScheme.surfaceContainerLow)
                        .padding(16.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Icon(FcIcons.Info, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(20.dp))
                    Text(
                        "asleep? put the headset on for a second. its wi-fi comes back when it wakes, then this finds it again by itself.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }

        Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth()) {
            Spacer(
                Modifier.fillMaxWidth().height(40.dp).background(
                    Brush.verticalGradient(listOf(Color.Transparent, MaterialTheme.colorScheme.surface)),
                ),
            )
            Row(
                Modifier
                    .fillMaxWidth()
                    .background(MaterialTheme.colorScheme.surface)
                    .navigationBarsPadding()
                    .padding(start = 16.dp, end = 16.dp, bottom = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                OutlinedButton(onClick = onPair, modifier = Modifier.weight(1f).height(56.dp)) { Text("pair again", maxLines = 1) }
                Button(onClick = { run++ }, modifier = Modifier.weight(1f).height(56.dp)) { Text("try again", maxLines = 1) }
            }
        }
    }
}

@Composable
private fun CheckRow(check: Check, index: Int, count: Int) {
    val scheme = MaterialTheme.colorScheme
    val status = LocalStatusColors.current
    Row(
        Modifier
            .fillMaxWidth()
            .clip(segmentShape(index, count, outer = 20.dp))
            .background(scheme.surfaceContainer)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        val (bg, fg) = when (check.result) {
            Result.Pass -> status.okContainer to status.ok
            Result.Fail -> scheme.errorContainer to scheme.onErrorContainer
            else -> scheme.surfaceContainerHighest to scheme.onSurfaceVariant
        }
        Box(Modifier.size(32.dp).clip(CircleShape).background(bg), contentAlignment = Alignment.Center) {
            when (check.result) {
                Result.Pass -> Icon(FcIcons.Check, contentDescription = "passed", tint = fg, modifier = Modifier.size(18.dp))
                Result.Fail -> Icon(FcIcons.Close, contentDescription = "failed", tint = fg, modifier = Modifier.size(18.dp))
                Result.Running -> LoadingIndicator(Modifier.size(28.dp))
                else -> Text("-", color = fg)
            }
        }
        Column(Modifier.weight(1f)) {
            Text(check.title, style = MaterialTheme.typography.titleSmall)
            Text(check.detail, style = MaterialTheme.typography.bodySmall, color = scheme.onSurfaceVariant)
        }
    }
}

private fun onWifi(context: Context): Boolean {
    val cm = context.getSystemService(ConnectivityManager::class.java) ?: return false
    val caps = cm.getNetworkCapabilities(cm.activeNetwork) ?: return false
    return caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) || caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
}
