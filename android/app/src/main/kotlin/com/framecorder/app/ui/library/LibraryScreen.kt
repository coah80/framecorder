package com.framecorder.app.ui.library

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearOutSlowInEasing
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.drag
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridItemScope
import androidx.compose.foundation.lazy.grid.LazyGridScope
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.PullToRefreshDefaults
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import com.framecorder.app.sync.MediaGallery
import com.framecorder.app.sync.SyncHub
import com.framecorder.app.sync.Transfer
import com.framecorder.app.ui.LocalShowNav
import com.framecorder.app.ui.common.ClipThumbnail
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.GlassPill
import com.framecorder.app.ui.common.PillRule
import com.framecorder.app.ui.common.ShapeIcon
import com.framecorder.app.ui.common.StatusBarBlur
import com.framecorder.app.ui.common.StatusDot
import com.framecorder.app.ui.common.day
import com.framecorder.app.ui.common.dayKey
import com.framecorder.app.ui.common.dotted
import com.framecorder.app.ui.common.isTrouble
import com.framecorder.app.ui.common.length
import com.framecorder.app.ui.common.rate
import com.framecorder.app.ui.common.sharedClip
import com.framecorder.app.ui.common.shortDay
import com.framecorder.app.ui.common.size
import com.framecorder.app.ui.common.stateText
import com.framecorder.app.ui.common.time
import com.framecorder.app.ui.theme.DataStyle
import com.framecorder.app.ui.theme.LocalStatusColors
import com.framecorder.core.Clip
import com.framecorder.core.FrameState
import com.framecorder.core.FrameStatus
import dev.chrisbanes.haze.HazeState
import dev.chrisbanes.haze.hazeSource
import dev.chrisbanes.haze.rememberHazeState
import java.time.LocalDate
import kotlin.math.roundToInt
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

private enum class Filter(val label: String) { All("all"), Clips("clips"), Recordings("recordings") }

@Composable
fun LibraryScreen(hub: SyncHub, onOpen: (String, List<String>) -> Unit, onFrame: (String?) -> Unit, takeLastSeen: () -> String?) {
    val clips by hub.clips.collectAsStateWithLifecycle()
    val missing by hub.missing.collectAsStateWithLifecycle()
    val frames by hub.frames.collectAsStateWithLifecycle()
    val transfer by hub.transfer.collectAsStateWithLifecycle()
    var filter by rememberSaveable { mutableStateOf(Filter.All) }
    var searching by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }

    // clips deleted in the gallery app go away from here too
    val lifecycle = LocalLifecycleOwner.current
    LaunchedEffect(clips.size) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) { hub.refreshMissing() }
    }

    val here = clips.filter { it.key !in missing }
    val words = query.trim().lowercase()
    val shown = here.filter {
        val kind = when (filter) {
            Filter.All -> true
            Filter.Clips -> it.kind == "clip"
            Filter.Recordings -> it.kind != "clip"
        }
        kind && (words.isEmpty() || words in "${time(it.created)} ${day(it.created)} ${it.kind} ${it.name}".lowercase())
    }
    val byDay = remember(shown) { shown.sortedByDescending { it.created }.groupBy { dayKey(it.created) }.toList() }
    val order = remember(byDay) { byDay.flatMap { (_, list) -> list.map { it.key } } }
    val trouble = frames.firstOrNull { it.state.isTrouble() }

    // where things sit in the grid, for the pill and the fast scroller
    val above = buildList {
        add("header")
        if (searching) add("search")
        if (transfer != null) add("transfer")
        if (trouble != null && transfer == null) add("trouble")
        if (here.isNotEmpty()) add("filter")
        if (here.isEmpty() && transfer == null) add("empty")
        if (shown.isEmpty() && here.isNotEmpty()) add("none")
    }
    val starts = remember(byDay, above.size) { byDay.runningFold(above.size) { at, (_, list) -> at + 1 + list.size }.dropLast(1) }

    val grid = rememberLazyGridState()
    val haze = rememberHazeState()
    val scope = rememberCoroutineScope()
    var refreshing by remember { mutableStateOf(false) }
    val pull = rememberPullToRefreshState()
    val search = {
        searching = !searching
        if (!searching) query = ""
        scope.launch { grid.animateScrollToItem(0) }
        Unit
    }
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    val density = LocalDensity.current
    // as many squares across as fit, at least three
    val columns = with(density) { (LocalWindowInfo.current.containerSize.width.toDp() - 24.dp) / 116.dp }.toInt().coerceAtLeast(3)
    // the pill takes over once the title has scrolled away
    val pinned by remember { derivedStateOf { grid.firstVisibleItemIndex > 0 || grid.firstVisibleItemScrollOffset > with(density) { 100.dp.toPx() } } }
    // how much of the list the pill and its blur cover, below the status bar
    val hidden = if (pinned) with(density) { 84.dp.roundToPx() } else 0
    // the day of the first clip you can actually see
    val active by remember(starts, hidden) {
        derivedStateOf {
            val first = grid.layoutInfo.visibleItemsInfo.firstOrNull { it.offset.y + it.size.height > hidden }?.index ?: grid.firstVisibleItemIndex
            starts.indexOfLast { it <= first }.coerceAtLeast(0)
        }
    }

    // a list too short to scroll can't bring the nav back, so it comes back by itself
    val showNav = LocalShowNav.current
    LaunchedEffect(grid) {
        snapshotFlow { grid.canScrollForward || grid.canScrollBackward }.collect { if (!it) showNav() }
    }

    // back from the player after swiping to another clip, that clip's tile comes into view so the video shrinks back into it
    LaunchedEffect(Unit) {
        val key = takeLastSeen() ?: return@LaunchedEffect
        val laid = snapshotFlow { grid.layoutInfo }.first { it.totalItemsCount > 0 }
        val tile = laid.visibleItemsInfo.firstOrNull { it.key == key }
        val bottom = laid.viewportSize.height - with(density) { (top + 100.dp).toPx() }
        if (tile != null && tile.offset.y >= hidden && tile.offset.y + tile.size.height <= bottom) return@LaunchedEffect
        val d = byDay.indexOfFirst { (_, list) -> list.any { it.key == key } }
        if (d < 0) return@LaunchedEffect
        val i = byDay[d].second.indexOfFirst { it.key == key }
        // the line above it goes to the top, which keeps the tile clear of the pill
        grid.scrollToItem(if (i < columns) (starts[d] - 1).coerceAtLeast(0) else starts[d] + 1 + i - columns)
    }

    // switching filters fades the old results out, then the new ones in, a row at a time
    val swap = remember { Animatable(1f) }
    var swapping by remember { mutableStateOf(false) }
    val pick: (Filter) -> Unit = { f ->
        if (f != filter && !swapping) {
            scope.launch {
                swapping = true
                showNav()
                swap.animateTo(0f, tween(90))
                filter = f
                swap.animateTo(1f, tween(420, easing = LinearOutSlowInEasing))
                swapping = false
            }
        }
    }

    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface)) {
        PullToRefreshBox(
            isRefreshing = refreshing,
            onRefresh = {
                hub.retryNow()
                refreshing = true
                scope.launch {
                    delay(1200)
                    refreshing = false
                }
            },
            state = pull,
            modifier = Modifier.fillMaxSize(),
            indicator = {
                PullToRefreshDefaults.LoadingIndicator(
                    state = pull,
                    isRefreshing = refreshing,
                    modifier = Modifier.align(Alignment.TopCenter).statusBarsPadding(),
                )
            },
        ) {
            LazyVerticalGrid(
                state = grid,
                columns = GridCells.Fixed(columns),
                contentPadding = PaddingValues(start = 12.dp, end = 12.dp, top = top, bottom = 140.dp),
                horizontalArrangement = Arrangement.spacedBy(3.dp),
                verticalArrangement = Arrangement.spacedBy(3.dp),
                modifier = Modifier.fillMaxSize().hazeSource(haze),
            ) {
                full("header") {
                    Header(
                        subtitle = if (here.isEmpty()) "nothing on this phone yet"
                        else "${here.size} ${if (here.size == 1) "video" else "videos"} · ${size(here.sumOf { it.size })} on this phone",
                        searchable = here.isNotEmpty(),
                        searching = searching,
                        paired = frames.isNotEmpty(),
                        onSearch = search,
                        onRetry = hub::retryNow,
                        onFrame = { onFrame(null) },
                    )
                }
                if (searching) full("search") { Search(query, onQuery = { query = it }, modifier = Modifier.animateItem()) }
                transfer?.let { t ->
                    full("transfer") {
                        Incoming(t, frames.firstOrNull { it.fingerprint == t.fingerprint }, onClick = { onFrame(t.fingerprint) }, modifier = Modifier.animateItem())
                    }
                }
                if (trouble != null && transfer == null) {
                    full("trouble") { Trouble(trouble, onClick = { onFrame(trouble.fingerprint) }, modifier = Modifier.animateItem()) }
                }
                if (here.isNotEmpty()) {
                    full("filter") {
                        FilterChips(filter, onPick = pick, modifier = Modifier.padding(start = 4.dp, bottom = 4.dp))
                    }
                }
                if (here.isEmpty() && transfer == null) {
                    full("empty") { Empty(paired = frames.isNotEmpty(), trouble = trouble, onFrame = { onFrame(trouble?.fingerprint) }) }
                }
                if (shown.isEmpty() && here.isNotEmpty()) {
                    full("none") {
                        Text(
                            when {
                                words.isNotEmpty() -> "nothing matches \"$query\"."
                                filter == Filter.Clips -> "no clips yet, just recordings."
                                else -> "no recordings yet, just clips."
                            },
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.animateItem().fadeThrough({ swap.value }, 0).padding(horizontal = 8.dp, vertical = 24.dp),
                        )
                    }
                }
                days(
                    byDay,
                    hub.gallery,
                    columns = columns,
                    swap = { swap.value },
                    swapping = swapping,
                    onOpen = { onOpen(it, order) },
                )
            }

            if (byDay.size > 1) {
                FastScroller(
                    days = byDay.map { (date, list) -> date to list.size },
                    starts = starts,
                    grid = grid,
                    active = active,
                    hidden = hidden,
                    modifier = Modifier
                        .align(Alignment.TopEnd)
                        .padding(top = top + 136.dp, bottom = 120.dp, end = 6.dp)
                        .fillMaxHeight(),
                )
            }
        }

        // reaches down past the pill while it's out
        val cover by animateDpAsState(if (pinned) 96.dp else 28.dp, MaterialTheme.motionScheme.defaultEffectsSpec(), label = "cover")
        val under by remember { derivedStateOf { grid.firstVisibleItemIndex > 0 || grid.firstVisibleItemScrollOffset > 0 } }
        if (under) {
            StatusBarBlur(haze, top + cover, shown = {
                if (grid.firstVisibleItemIndex > 0) 1f else (grid.firstVisibleItemScrollOffset / with(density) { 24.dp.toPx() }).coerceIn(0f, 1f)
            })
        }

        AnimatedVisibility(
            visible = pinned && byDay.isNotEmpty(),
            enter = fadeIn() + scaleIn(MaterialTheme.motionScheme.defaultSpatialSpec(), initialScale = 0.8f) +
                slideInVertically(MaterialTheme.motionScheme.defaultSpatialSpec()) { -it },
            exit = fadeOut() + scaleOut(targetScale = 0.8f) + slideOutVertically { -it },
            modifier = Modifier.align(Alignment.TopCenter).statusBarsPadding().padding(top = 8.dp),
        ) {
            DayPill(
                day = byDay.getOrNull(active)?.first?.let { day(it) } ?: "",
                filter = filter,
                transfer = transfer,
                haze = haze,
                onClick = { scope.launch { grid.animateScrollToItem(0) } },
            )
        }
    }
}

private fun LazyGridScope.full(key: String, content: @Composable LazyGridItemScope.() -> Unit) {
    item(key = key, span = { GridItemSpan(maxLineSpan) }) { content() }
}

private fun LazyGridScope.days(
    byDay: List<Pair<LocalDate, List<Clip>>>,
    gallery: MediaGallery,
    columns: Int,
    swap: () -> Float,
    swapping: Boolean,
    onOpen: (String) -> Unit,
) {
    var row = 0
    for ((date, list) in byDay) {
        val at = row
        full("day-$date") {
            Text(
                day(list.first().created),
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = settles(swapping).fadeThrough(swap, at).padding(start = 4.dp, top = 18.dp, bottom = 6.dp),
            )
        }
        itemsIndexed(list, key = { _, it -> it.key }) { i, clip ->
            ClipTile(
                clip,
                gallery,
                shape = tileShape(i, list.size, columns),
                onClick = { onOpen(clip.key) },
                modifier = settles(swapping).fadeThrough(swap, at + 1 + i / columns),
            )
        }
        row += 1 + (list.size + columns - 1) / columns
    }
}

/** Big corners where a day's block of tiles has its outside corners, small ones between tiles, so each day reads as one shape. */
private fun tileShape(i: Int, n: Int, columns: Int): RoundedCornerShape {
    val row = i / columns
    val col = i % columns
    val lastRow = (n - 1) / columns
    val rowEnd = if (row == lastRow) (n - 1) % columns else columns - 1
    // a short last row leaves the corner above its end showing too
    val stepped = row == lastRow - 1 && col == columns - 1 && (n - 1) % columns != columns - 1
    fun corner(outside: Boolean) = if (outside) 18.dp else 3.dp
    return RoundedCornerShape(
        topStart = corner(row == 0 && col == 0),
        topEnd = corner(row == 0 && col == rowEnd),
        bottomStart = corner(row == lastRow && col == 0),
        bottomEnd = corner((row == lastRow && col == rowEnd) || stepped),
    )
}

/** Moves to its new place on a spring, except while filters switch, when everything fades instead. */
@Composable
private fun LazyGridItemScope.settles(swapping: Boolean): Modifier = Modifier.animateItem(
    fadeInSpec = if (swapping) null else MaterialTheme.motionScheme.defaultEffectsSpec(),
    placementSpec = if (swapping) null else MaterialTheme.motionScheme.defaultSpatialSpec(),
    fadeOutSpec = if (swapping) null else MaterialTheme.motionScheme.fastEffectsSpec(),
)

/** Fades and rises in with the new results, a beat after the row above it. */
private fun Modifier.fadeThrough(progress: () -> Float, row: Int): Modifier = graphicsLayer {
    val delay = row.coerceAtMost(6) * 0.08f
    val p = ((progress() - delay) / (1f - delay)).coerceIn(0f, 1f)
    alpha = p
    translationY = (1f - p) * 20.dp.toPx()
}

/** The big title, before the list scrolls; the pill takes over after. */
@Composable
private fun Header(
    subtitle: String,
    searchable: Boolean,
    searching: Boolean,
    paired: Boolean,
    onSearch: () -> Unit,
    onRetry: () -> Unit,
    onFrame: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth().padding(start = 4.dp)) {
        Row(Modifier.fillMaxWidth().height(56.dp).offset(x = 12.dp), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.CenterVertically) {
            if (searchable) {
                IconButton(onClick = onSearch) {
                    Icon(if (searching) FcIcons.Close else FcIcons.Search, contentDescription = if (searching) "stop searching" else "search")
                }
            }
            Box {
                IconButton(onClick = { menu = true }) { Icon(FcIcons.More, contentDescription = "more") }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    DropdownMenuItem(
                        text = { Text("try again now") },
                        leadingIcon = { Icon(FcIcons.Retry, contentDescription = null) },
                        onClick = {
                            menu = false
                            onRetry()
                        },
                    )
                    DropdownMenuItem(
                        text = { Text(if (paired) "your frame" else "pair a frame") },
                        leadingIcon = { Icon(FcIcons.Headset, contentDescription = null) },
                        onClick = {
                            menu = false
                            onFrame()
                        },
                    )
                }
            }
        }
        Text(dotted("framecorder"), style = MaterialTheme.typography.displaySmall)
        Text(
            subtitle,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 2.dp, bottom = 6.dp),
        )
    }
}

/** The day you're looking at once the page has scrolled, and the download while one runs. Tapping it goes back up. */
@Composable
private fun DayPill(day: String, filter: Filter, transfer: Transfer?, haze: HazeState, onClick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    GlassPill(haze, onClick = onClick, onClickLabel = "back to the top") {
        Text(day, style = MaterialTheme.typography.titleMedium.copy(fontSize = 17.sp, fontWeight = FontWeight.SemiBold), maxLines = 1)
        if (filter != Filter.All) Text(filter.label, style = MaterialTheme.typography.titleMedium, color = scheme.onSurfaceVariant, maxLines = 1)
        if (transfer != null) {
            PillRule()
            Icon(FcIcons.Down, contentDescription = null, tint = scheme.primary, modifier = Modifier.size(16.dp))
            Text("${(transfer.fraction * 100).toInt()}%", style = DataStyle.copy(fontSize = 15.sp), color = scheme.onSurface)
        }
    }
}

/** A clip as a square of its picture, with its length up in the corner the way a photos app marks videos. */
@Composable
private fun ClipTile(clip: Clip, gallery: MediaGallery, shape: RoundedCornerShape, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val recording = clip.kind != "clip"
    val said = listOfNotNull(if (recording) "recording" else "clip", "${day(clip.created)}, ${time(clip.created)}", clip.durationS?.let(::length))
    Box(
        modifier
            .aspectRatio(1f)
            // the whole tile goes to the player and back, corners and all, so nothing pops in after
            .sharedClip(clip.key, shape)
            .clickable(onClickLabel = "play", onClick = onClick)
            .semantics { contentDescription = said.joinToString(", ") },
    ) {
        ClipThumbnail(clip, gallery, Modifier.fillMaxSize())
        // just enough shade up top for the length to read on a bright picture
        Box(
            Modifier
                .align(Alignment.TopCenter)
                .fillMaxWidth()
                .height(36.dp)
                .background(Brush.verticalGradient(listOf(Color.Black.copy(alpha = 0.35f), Color.Transparent))),
        )
        Row(
            Modifier.align(Alignment.TopEnd).padding(horizontal = 8.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            if (recording) StatusDot(LocalStatusColors.current.rec, size = 7.dp)
            clip.durationS?.let { Text(length(it), style = TileLength) }
            Icon(FcIcons.Play, contentDescription = null, tint = Color.White, modifier = Modifier.size(12.dp))
        }
    }
}

private val TileLength = DataStyle.copy(
    fontSize = 12.sp,
    lineHeight = 14.sp,
    color = Color.White,
    shadow = Shadow(Color.Black.copy(alpha = 0.6f), blurRadius = 6f),
)

/** What's coming over right now. */
@Composable
private fun Incoming(t: Transfer, frame: FrameStatus?, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val progress by animateFloatAsState(t.fraction, MaterialTheme.motionScheme.defaultEffectsSpec(), label = "download")
    val what = if (t.id.startsWith("r")) "a recording" else "a clip"
    Column(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(24.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerHigh)
            .clickable(onClick = onClick)
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                "getting $what from ${frame?.name ?: "your frame"}",
                style = MaterialTheme.typography.titleSmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            Text("${(t.fraction * 100).toInt()}%", style = DataStyle.copy(color = MaterialTheme.colorScheme.onSurface))
        }
        LinearWavyProgressIndicator(progress = { progress }, modifier = Modifier.fillMaxWidth())
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                "${size(t.done)} of ${size(t.total)}" + if (t.queued > 0) " · ${t.queued} more after it" else "",
                style = DataStyle,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                modifier = Modifier.weight(1f),
            )
            if (t.bytesPerSecond > 0) {
                Icon(FcIcons.Down, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(14.dp))
                Spacer(Modifier.width(3.dp))
                Text(rate(t.bytesPerSecond), style = DataStyle, color = MaterialTheme.colorScheme.primary)
            }
        }
    }
}

/** One line when a frame can't be reached, so a quiet library has a reason. */
@Composable
private fun Trouble(frame: FrameStatus, onClick: () -> Unit, modifier: Modifier = Modifier) {
    Row(
        modifier
            .fillMaxWidth()
            .clip(CircleShape)
            .background(MaterialTheme.colorScheme.surfaceContainer)
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        StatusDot(MaterialTheme.colorScheme.error)
        Text(
            "${frame.name}: ${stateText(frame)}",
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        Text("check", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
    }
}

@Composable
private fun Search(query: String, onQuery: (String) -> Unit, modifier: Modifier = Modifier) {
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    TextField(
        value = query,
        onValueChange = onQuery,
        singleLine = true,
        placeholder = { Text("search by time, day or name") },
        leadingIcon = { Icon(FcIcons.Search, contentDescription = null) },
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        shape = CircleShape,
        colors = TextFieldDefaults.colors(
            focusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
            unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
            focusedIndicatorColor = Color.Transparent,
            unfocusedIndicatorColor = Color.Transparent,
        ),
        modifier = modifier.fillMaxWidth().focusRequester(focus),
    )
}

/** Clips or recordings only; tapping the one that's on goes back to everything. */
@Composable
private fun FilterChips(filter: Filter, onPick: (Filter) -> Unit, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        for (option in listOf(Filter.Clips, Filter.Recordings)) {
            val on = filter == option
            FilterChip(
                selected = on,
                onClick = { onPick(if (on) Filter.All else option) },
                label = { Text(option.label) },
                leadingIcon = if (on) {
                    { Icon(FcIcons.Check, contentDescription = null, modifier = Modifier.size(FilterChipDefaults.IconSize)) }
                } else {
                    null
                },
                shape = CircleShape,
                colors = FilterChipDefaults.filterChipColors(
                    labelColor = scheme.onSurfaceVariant,
                    selectedContainerColor = scheme.secondaryContainer,
                    selectedLabelColor = scheme.onSecondaryContainer,
                    selectedLeadingIconColor = scheme.onSecondaryContainer,
                ),
                border = FilterChipDefaults.filterChipBorder(enabled = true, selected = on, borderColor = scheme.outlineVariant),
            )
        }
    }
}

/**
 * Fast scrolling by day: a segment per day, as tall as its share of the
 * list, notched where the date changes, the one you're on lit. It shows
 * while the list moves; dragging it jumps there, with the day in a bubble.
 */
@Composable
private fun FastScroller(
    days: List<Pair<LocalDate, Int>>,
    starts: List<Int>,
    grid: LazyGridState,
    /** The day you're on. */
    active: Int,
    /** Pixels at the top of the list the pill covers. */
    hidden: Int,
    modifier: Modifier = Modifier,
) {
    val scheme = MaterialTheme.colorScheme
    val density = LocalDensity.current
    var dragging by remember { mutableStateOf(false) }
    var shown by remember { mutableStateOf(false) }
    var target by remember { mutableIntStateOf(-1) }
    val moving = grid.isScrollInProgress
    val long = grid.canScrollForward || grid.canScrollBackward
    LaunchedEffect(moving, dragging, long) {
        if ((moving || dragging) && long) {
            shown = true
        } else {
            delay(1400)
            shown = false
        }
    }
    LaunchedEffect(target) { if (target >= 0) grid.scrollToItem(target, -hidden) }
    val alpha by animateFloatAsState(if (shown) 1f else 0f, MaterialTheme.motionScheme.defaultEffectsSpec(), label = "scroller")

    BoxWithConstraints(modifier.width(120.dp).graphicsLayer { this.alpha = alpha }) {
        val height = constraints.maxHeight.toFloat()
        val total = days.sumOf { it.second }.coerceAtLeast(1)
        val gap = with(density) { 4.dp.toPx() }.coerceAtMost(height / days.size / 3)
        val unit = (height - gap * (days.size - 1)) / total
        val tops = days.runningFold(0f) { y, d -> y + d.second * unit + gap }

        fun jump(y: Float) {
            val i = tops.indexOfLast { it <= y }.coerceIn(0, days.lastIndex)
            val n = days[i].second
            val within = ((y - tops[i]) / (n * unit)).coerceIn(0f, 0.999f)
            target = starts[i] + if (within < 0.08f) 0 else 1 + (within * n).toInt()
        }

        days.forEachIndexed { i, (date, n) ->
            key(date) {
                val on = i == active
                val width by animateDpAsState(if (on) 6.dp else 4.dp, MaterialTheme.motionScheme.fastSpatialSpec(), label = "segment")
                Box(
                    Modifier
                        .align(Alignment.TopEnd)
                        .offset { IntOffset(0, tops[i].roundToInt()) }
                        .width(width)
                        .height(with(density) { (n * unit).coerceAtLeast(2f).toDp() })
                        .clip(RoundedCornerShape(3.dp))
                        .background(if (on) scheme.primary else scheme.onSurfaceVariant.copy(alpha = 0.32f)),
                )
            }
        }

        if (dragging && active in days.indices) {
            val center = tops[active] + days[active].second * unit / 2
            val y by animateFloatAsState(center, MaterialTheme.motionScheme.fastSpatialSpec(), label = "bubble")
            Text(
                shortDay(days[active].first),
                style = MaterialTheme.typography.titleSmall,
                color = scheme.onPrimaryContainer,
                maxLines = 1,
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .offset { IntOffset(-18.dp.roundToPx(), (y - 20.dp.toPx()).roundToInt()) }
                    .clip(RoundedCornerShape(20.dp))
                    .background(scheme.primaryContainer)
                    .padding(horizontal = 16.dp, vertical = 8.dp),
            )
        }

        // only there while it shows, so it never takes a tap meant for a clip
        if (shown) {
            Box(
                Modifier.align(Alignment.TopEnd).width(32.dp).fillMaxHeight().pointerInput(days, starts, height) {
                    awaitEachGesture {
                        val down = awaitFirstDown()
                        dragging = true
                        jump(down.position.y)
                        drag(down.id) {
                            it.consume()
                            jump(it.position.y)
                        }
                        dragging = false
                    }
                },
            )
        }
    }
}

/** Nothing here yet: says why, and where to go about it. */
@Composable
private fun Empty(paired: Boolean, trouble: FrameStatus?, onFrame: () -> Unit) {
    Column(
        Modifier.fillMaxWidth().heightIn(min = 440.dp).padding(horizontal = 16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        ShapeIcon(
            FcIcons.Library,
            shape = MaterialShapes.Clover4Leaf,
            size = 120.dp,
            iconSize = 44.dp,
            container = MaterialTheme.colorScheme.surfaceContainerHigh,
            content = MaterialTheme.colorScheme.primary,
        )
        Spacer(Modifier.height(24.dp))
        Text(
            when {
                !paired -> "nothing here yet"
                trouble != null -> "waiting for ${trouble.name}"
                else -> "all caught up"
            },
            style = MaterialTheme.typography.headlineSmall,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            when {
                !paired -> "pair your frame and every clip and recording lands here, straight over your wi-fi."
                trouble?.state == FrameState.UNPAIRED -> "it doesn't know this phone anymore. pair it again from the frame tab."
                trouble?.state == FrameState.FULL -> "this phone is out of space. free some up and the rest comes over by itself."
                trouble != null -> "can't reach it right now. anything it saves waits there until it's back."
                else -> "save a clip or a recording on your frame and it shows up here a moment later."
            },
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        AnimatedVisibility(!paired || trouble != null) {
            Button(onClick = onFrame, modifier = Modifier.padding(top = 24.dp).height(48.dp)) {
                Icon(FcIcons.Headset, contentDescription = null, modifier = Modifier.size(20.dp))
                Spacer(Modifier.width(8.dp))
                Text(if (!paired) "pair your frame" else "check the connection")
            }
        }
    }
}
