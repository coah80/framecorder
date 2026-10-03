package com.framecorder.app.ui

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.SharedTransitionLayout
import androidx.compose.animation.SharedTransitionScope
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.FloatingToolbarDefaults
import androidx.compose.material3.FloatingToolbarDefaults.floatingToolbarVerticalNestedScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation3.runtime.entryProvider
import androidx.navigation3.ui.NavDisplay
import com.framecorder.app.Prefs
import com.framecorder.app.sync.SyncHub
import com.framecorder.app.sync.SyncService
import com.framecorder.app.ui.common.FloatingNav
import com.framecorder.app.ui.diagnose.DiagnoseScreen
import com.framecorder.app.ui.frame.FrameScreen
import com.framecorder.app.ui.library.LibraryScreen
import com.framecorder.app.ui.pair.PairScreen
import com.framecorder.app.ui.player.PlayerScreen
import com.framecorder.app.ui.settings.SettingsScreen

/** Where the app is. The first three are the tabs in the floating nav. */
sealed interface Screen {
    data object Library : Screen
    data class Frame(val fingerprint: String? = null) : Screen
    data object Settings : Screen
    /** [among] is what a swipe goes through, in order; empty means every clip on the phone. */
    data class Player(val key: String, val among: List<String> = emptyList()) : Screen
    data class Pair(val link: String? = null, val replaces: String? = null) : Screen
    data class Diagnose(val fingerprint: String) : Screen
}

private fun Screen.isTab() = this is Screen.Library || this is Screen.Frame || this is Screen.Settings

/** For the clip thumbnail that grows into the player. */
val LocalSharedScope = staticCompositionLocalOf<SharedTransitionScope?> { null }

/** Brings the floating nav back, for screens that can't be scrolled to do it. */
val LocalShowNav = staticCompositionLocalOf<() -> Unit> { {} }

private val tabChange = NavDisplay.transitionSpec { fadeIn(tween(220, 90)) + scaleIn(tween(220, 90), 0.96f) togetherWith fadeOut(tween(90)) } +
    NavDisplay.popTransitionSpec { fadeIn(tween(220, 90)) togetherWith fadeOut(tween(90)) }

@Composable
fun App(hub: SyncHub, prefs: Prefs, link: String?, clip: String?, onIntentHandled: () -> Unit) {
    val backStack = remember {
        mutableStateListOf<Screen>(if (hub.frames.value.isEmpty()) Screen.Frame() else Screen.Library)
    }
    var navShown by remember { mutableStateOf(true) }
    // the clip the player was on, so the library can bring its tile into view on the way back
    var lastSeen by remember { mutableStateOf<String?>(null) }

    fun go(tab: Screen) {
        backStack.clear()
        backStack.add(tab)
        navShown = true
        lastSeen = null
    }

    fun push(screen: Screen) = backStack.add(screen)
    fun pop() {
        if (backStack.size > 1) backStack.removeAt(backStack.lastIndex)
    }

    LaunchedEffect(link, clip) {
        if (link != null) push(Screen.Pair(link))
        if (clip != null) push(Screen.Player(clip))
        if (link != null || clip != null) onIntentHandled()
    }

    // after the first pairing: keep syncing with the app closed, and ask to post a note per clip
    val context = LocalContext.current
    val frames by hub.frames.collectAsStateWithLifecycle()
    val askNotifications = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }
    var hadFrames by rememberSaveable { mutableStateOf(frames.isNotEmpty()) }
    LaunchedEffect(frames.isNotEmpty()) {
        if (frames.isEmpty() || hadFrames) return@LaunchedEffect
        hadFrames = true
        if (prefs.background.value) runCatching { SyncService.start(context) }
        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && !granted) askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
    }

    // pairing from the frame tab scrolls the nav away; it comes back with the new frame
    LaunchedEffect(frames.size) { navShown = true }

    val top = backStack.lastOrNull() ?: Screen.Library
    BackHandler(enabled = backStack.size == 1 && (top is Screen.Frame || top is Screen.Settings)) { go(Screen.Library) }

    // a Surface so every screen gets the right text color, Scaffold or not
    Surface(color = MaterialTheme.colorScheme.surface, contentColor = MaterialTheme.colorScheme.onSurface) {
        SharedTransitionLayout(Modifier.fillMaxSize()) {
            CompositionLocalProvider(LocalSharedScope provides this, LocalShowNav provides { navShown = true }) {
                Box(Modifier.fillMaxSize()) {
                    NavDisplay(
                        backStack = backStack,
                        onBack = ::pop,
                        sharedTransitionScope = this@SharedTransitionLayout,
                        modifier = Modifier.floatingToolbarVerticalNestedScroll(
                            expanded = navShown,
                            onExpand = { navShown = true },
                            onCollapse = { navShown = false },
                        ),
                        transitionSpec = {
                            (slideInHorizontally(tween(380)) { it / 6 } + fadeIn(tween(220, 90))) togetherWith
                                (slideOutHorizontally(tween(380)) { -it / 10 } + fadeOut(tween(120)))
                        },
                        popTransitionSpec = {
                            (slideInHorizontally(tween(380)) { -it / 10 } + fadeIn(tween(220, 90))) togetherWith
                                (slideOutHorizontally(tween(380)) { it / 6 } + fadeOut(tween(120)))
                        },
                        predictivePopTransitionSpec = {
                            fadeIn(tween(200)) togetherWith (fadeOut(tween(200)) + scaleOut(targetScale = 0.9f))
                        },
                        entryProvider = entryProvider {
                            entry<Screen.Library>(metadata = tabChange) {
                                LibraryScreen(
                                    hub = hub,
                                    onOpen = { key, among -> push(Screen.Player(key, among)) },
                                    onFrame = { go(Screen.Frame(it)) },
                                    takeLastSeen = { lastSeen.also { lastSeen = null } },
                                )
                            }
                            entry<Screen.Frame>(metadata = tabChange) { key ->
                                FrameScreen(
                                    hub = hub,
                                    fingerprint = key.fingerprint,
                                    onDiagnose = { push(Screen.Diagnose(it)) },
                                    onPairAnother = { push(Screen.Pair()) },
                                )
                            }
                            entry<Screen.Settings>(metadata = tabChange) {
                                SettingsScreen(
                                    hub = hub,
                                    prefs = prefs,
                                    onFrame = { go(Screen.Frame(it)) },
                                    onPair = { push(Screen.Pair()) },
                                )
                            }
                            entry<Screen.Player> { key ->
                                PlayerScreen(hub = hub, key = key.key, among = key.among, onPage = { lastSeen = it }, onBack = ::pop)
                            }
                            entry<Screen.Pair> { key ->
                                PairScreen(
                                    hub = hub,
                                    link = key.link,
                                    replaces = key.replaces,
                                    onBack = ::pop,
                                    onPaired = { go(Screen.Frame(it.fingerprint)) },
                                )
                            }
                            entry<Screen.Diagnose> { key ->
                                DiagnoseScreen(
                                    hub = hub,
                                    fingerprint = key.fingerprint,
                                    onBack = ::pop,
                                    onPair = { push(Screen.Pair(replaces = key.fingerprint)) },
                                )
                            }
                        },
                    )

                    if (top.isTab()) {
                        Box(
                            Modifier.align(Alignment.BottomCenter).fillMaxWidth().height(132.dp).background(
                                Brush.verticalGradient(
                                    0f to Color.Transparent,
                                    0.55f to MaterialTheme.colorScheme.surface.copy(alpha = 0.78f),
                                    1f to MaterialTheme.colorScheme.surface,
                                ),
                            ),
                        )
                    }
                    AnimatedVisibility(
                        visible = top.isTab() && navShown,
                        modifier = Modifier.align(Alignment.BottomCenter),
                        enter = slideInVertically(MaterialTheme.motionScheme.defaultSpatialSpec()) { it * 2 } + fadeIn(),
                        exit = slideOutVertically(MaterialTheme.motionScheme.fastSpatialSpec()) { it * 2 } + fadeOut(),
                    ) {
                        FloatingNav(
                            current = top,
                            frames = hub.frames,
                            busy = hub.busy,
                            onTab = ::go,
                            modifier = Modifier.navigationBarsPadding().padding(bottom = FloatingToolbarDefaults.ScreenOffset),
                        )
                    }
                }
            }
        }
    }
}
