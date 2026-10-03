package com.framecorder.app.ui.player

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.IntentSenderRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.compose.ContentFrame
import androidx.media3.ui.compose.SURFACE_TYPE_TEXTURE_VIEW
import com.framecorder.app.sync.SyncHub
import com.framecorder.app.ui.common.ClipThumbnail
import com.framecorder.app.ui.common.FcIcons
import com.framecorder.app.ui.common.PlayerCorner
import com.framecorder.app.ui.common.day
import com.framecorder.app.ui.common.length
import com.framecorder.app.ui.common.sharedClip
import com.framecorder.app.ui.common.size
import com.framecorder.app.ui.common.time
import com.framecorder.app.ui.theme.DataStyle
import com.framecorder.core.Clip
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** What the file says about itself. */
private data class VideoInfo(val width: Int, val height: Int, val fps: Int?, val codec: String?) {
    val aspect: Float get() = if (width > 0 && height > 0) width.toFloat() / height else 16f / 9f
    val shape: String
        get() = when {
            abs(aspect - 16f / 9f) < 0.05f -> "16:9"
            abs(aspect - 1f) < 0.05f -> "1:1"
            abs(aspect - 9f / 16f) < 0.05f -> "9:16"
            aspect > 1.9f -> "both eyes"
            else -> "${width}×$height"
        }
}

@Composable
fun PlayerScreen(hub: SyncHub, key: String, among: List<String>, onPage: (String) -> Unit, onBack: () -> Unit) {
    val clips by hub.clips.collectAsStateWithLifecycle()
    val missing by hub.missing.collectAsStateWithLifecycle()
    val frames by hub.frames.collectAsStateWithLifecycle()
    // what a swipe goes through: the library's clips in its order, or all of them when opened from elsewhere
    val pages = remember(clips, missing, among) {
        val here = clips.filter { it.key !in missing }
        if (among.isEmpty()) here.sortedByDescending { it.created } else here.associateBy { it.key }.let { byKey -> among.mapNotNull { byKey[it] } }
    }
    var showing by rememberSaveable { mutableStateOf(key) }
    val at = pages.indexOfFirst { it.key == showing }
    if (at < 0) {
        Box(Modifier.fillMaxSize().statusBarsPadding(), contentAlignment = Alignment.Center) {
            Text("that clip isn't on this phone anymore", color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        return
    }
    val pager = rememberPagerState(initialPage = at) { pages.size }
    val clip = pages[pager.settledPage.coerceIn(pages.indices)]
    LaunchedEffect(clip.key) {
        showing = clip.key
        onPage(clip.key)
    }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val infos = remember { mutableStateMapOf<String, VideoInfo>() }

    val player = remember { ExoPlayer.Builder(context).build() }
    DisposableEffect(clip.location) {
        player.setMediaItem(MediaItem.fromUri(clip.location))
        player.prepare()
        player.playWhenReady = true
        onDispose { }
    }
    DisposableEffect(Unit) { onDispose { player.release() } }
    // pauses when the app goes to the background, and stays paused
    val lifecycle = LocalLifecycleOwner.current
    LaunchedEffect(player) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            try {
                kotlinx.coroutines.awaitCancellation()
            } finally {
                player.pause()
            }
        }
    }

    var playing by remember { mutableStateOf(false) }
    var ended by remember { mutableStateOf(false) }
    var position by remember { mutableLongStateOf(0L) }
    var duration by remember { mutableLongStateOf(0L) }
    var seeking by remember { mutableStateOf(false) }

    // a trim belongs to the clip it was made on
    var trimming by rememberSaveable(clip.key) { mutableStateOf(false) }
    var trimStart by rememberSaveable(clip.key) { mutableLongStateOf(0L) }
    var trimEnd by rememberSaveable(clip.key) { mutableLongStateOf(-1L) }
    var scrubbing by remember { mutableStateOf(false) }
    var exporting by remember { mutableStateOf<Float?>(null) }
    val trimmed = trimming && trimEnd > 0 && (trimStart > 0 || trimEnd < duration)
    val strip by produceState(emptyList<ImageBitmap>(), clip.location, trimming && duration > 0) {
        if (trimming && duration > 0 && value.isEmpty()) value = withContext(Dispatchers.IO) { filmstrip(context, Uri.parse(clip.location), duration) }
    }

    LaunchedEffect(player, clip.key) {
        while (true) {
            playing = player.isPlaying || (player.playWhenReady && player.playbackState == Player.STATE_BUFFERING)
            ended = player.playbackState == Player.STATE_ENDED
            position = player.currentPosition
            duration = player.duration.coerceAtLeast(0L)
            // while trimming it plays just the part that's kept, round and round
            val running = player.isPlaying || (ended && player.playWhenReady)
            if (trimming && !scrubbing && trimEnd > 0 && running && (ended || position >= trimEnd || position < trimStart - 250)) {
                player.seekTo(trimStart)
            }
            delay(50)
        }
    }
    val toggle = {
        when {
            player.isPlaying -> player.pause()
            player.playbackState == Player.STATE_ENDED -> {
                player.seekTo(if (trimming) trimStart else 0L)
                player.play()
            }
            else -> player.play()
        }
    }

    val deleter = rememberLauncherForActivityResult(ActivityResultContracts.StartIntentSenderForResult()) { result ->
        if (result.resultCode == Activity.RESULT_OK) {
            scope.launch {
                hub.refreshMissing()
                onBack()
            }
        }
    }

    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainerLowest)) {
        Column(Modifier.fillMaxSize().statusBarsPadding().navigationBarsPadding().padding(bottom = 104.dp)) {
            Row(Modifier.padding(horizontal = 4.dp).fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onBack) { Icon(FcIcons.Back, contentDescription = "back") }
            }
            // swiping goes to the next clip; the player only ever shows the one that's settled
            HorizontalPager(
                state = pager,
                key = { pages[it].key },
                pageSpacing = 16.dp,
                userScrollEnabled = !trimming && exporting == null,
                modifier = Modifier.weight(1f).fillMaxWidth(),
            ) { page ->
                val c = pages[page]
                val current = c.key == clip.key
                LaunchedEffect(c.location) {
                    if (c.key !in infos) probe(context, Uri.parse(c.location))?.let { infos[c.key] = it }
                }
                // as big as fits and centered, in whatever room the controls leave; the thumbnail has the shape until the file's been read
                val guess = remember(c.location) { hub.gallery.cachedThumbnail(c.location)?.let { it.width.toFloat() / it.height } }
                val aspect by animateFloatAsState(infos[c.key]?.aspect ?: guess ?: (16f / 9f), MaterialTheme.motionScheme.defaultSpatialSpec<Float>(), label = "aspect")
                BoxWithConstraints(Modifier.fillMaxSize().padding(horizontal = 8.dp), contentAlignment = Alignment.Center) {
                    val width = minOf(maxWidth, maxHeight * aspect)
                    Box(
                        Modifier
                            .size(width, width / aspect)
                            .then(if (current) Modifier.sharedClip(c.key) else Modifier.clip(RoundedCornerShape(PlayerCorner)))
                            .background(Color.Black)
                            .clickable(
                                enabled = current,
                                onClickLabel = when {
                                    playing -> "pause"
                                    ended -> "play again"
                                    else -> "play"
                                },
                                onClick = toggle,
                            ),
                    ) {
                        // the thumbnail covers the video until its first frame, so switching clips never flashes black
                        val cover = @Composable { ClipThumbnail(c, hub.gallery, Modifier.fillMaxSize()) }
                        if (current) {
                            ContentFrame(player = player, surfaceType = SURFACE_TYPE_TEXTURE_VIEW, modifier = Modifier.fillMaxSize(), shutter = cover)
                            PlayMark(visible = !playing, ended = ended, modifier = Modifier.align(Alignment.Center))
                        } else {
                            cover()
                        }
                    }
                }
            }

            Column(Modifier.padding(horizontal = 20.dp).padding(top = 12.dp)) {
                val slider = remember { SliderState() }
                if (!seeking) slider.value = if (duration > 0) position.toFloat() / duration else 0f
                Slider(
                    state = slider,
                    onValueChange = {
                        seeking = true
                        slider.value = it
                    },
                    onValueChangeFinished = {
                        player.seekTo((slider.value * duration).toLong())
                        seeking = false
                    },
                    enabled = duration > 0,
                )
                Row {
                    Text(length(position / 1000.0), style = DataStyle, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.weight(1f))
                    Text(length(duration / 1000.0), style = DataStyle, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }

            AnimatedVisibility(trimming && duration > 0, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
                TrimStrip(
                    frames = strip,
                    durationMs = duration,
                    start = trimStart,
                    end = if (trimEnd > 0) trimEnd else duration,
                    onChange = { s, e ->
                        trimStart = s
                        trimEnd = e
                    },
                    onMove = {
                        scrubbing = true
                        player.pause()
                        player.seekTo(it)
                    },
                    onDone = {
                        scrubbing = false
                        player.seekTo(trimStart)
                        player.play()
                    },
                    modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 14.dp),
                )
            }

            AnimatedContent(
                clip,
                contentKey = { it.key },
                transitionSpec = { fadeIn(tween(220, 90)) togetherWith fadeOut(tween(90)) },
                label = "details",
            ) { c -> Details(c, infos[c.key], frames.firstOrNull { it.fingerprint == c.host }?.name) }
        }

        Row(
            Modifier
                .align(Alignment.BottomCenter)
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
                    IconToggleButton(
                        checked = trimming,
                        onCheckedChange = {
                            trimming = it
                            if (it && trimEnd < 0) trimEnd = duration
                            if (it) player.seekTo(trimStart)
                        },
                        shapes = IconButtonDefaults.toggleableShapes(),
                        colors = IconButtonDefaults.iconToggleButtonColors(
                            checkedContainerColor = MaterialTheme.colorScheme.secondaryContainer,
                            checkedContentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                        ),
                    ) { Icon(FcIcons.Trim, contentDescription = "trim") }
                    IconButton(onClick = { open(context, clip) }) { Icon(FcIcons.Open, contentDescription = "open in another app") }
                    IconButton(onClick = {
                        val request = hub.gallery.deleteRequest(listOf(clip.location))
                        if (request != null) {
                            deleter.launch(IntentSenderRequest.Builder(request.intentSender).build())
                        } else {
                            scope.launch {
                                hub.refreshMissing()
                                onBack()
                            }
                        }
                    }) { Icon(FcIcons.Delete, contentDescription = "delete from this phone") }
                }
            }
            ExtendedFloatingActionButton(
                onClick = {
                    when {
                        exporting != null -> {}
                        !trimmed -> share(context, clip)
                        else -> scope.launch {
                            exporting = 0f
                            try {
                                val file = exportTrim(context, Uri.parse(clip.location), clip.name, trimStart, trimEnd) { exporting = it }
                                shareTrim(context, file, recording = clip.kind != "clip")
                            } catch (e: CancellationException) {
                                throw e
                            } catch (e: Exception) {
                                Toast.makeText(context, "couldn't trim it: ${e.message}", Toast.LENGTH_LONG).show()
                            } finally {
                                exporting = null
                            }
                        }
                    }
                },
                icon = {
                    if (exporting != null) LoadingIndicator(Modifier.size(28.dp), color = MaterialTheme.colorScheme.onPrimaryContainer)
                    else Icon(FcIcons.Share, contentDescription = null)
                },
                text = {
                    Text(
                        when {
                            exporting != null -> "trimming ${((exporting ?: 0f) * 100).toInt()}%"
                            trimmed -> "share ${span(trimEnd - trimStart)}"
                            else -> "share"
                        },
                        style = MaterialTheme.typography.titleMedium,
                    )
                },
                containerColor = MaterialTheme.colorScheme.primaryContainer,
                contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                shape = RoundedCornerShape(20.dp),
                modifier = Modifier.height(64.dp).animateContentSize(MaterialTheme.motionScheme.fastSpatialSpec()),
            )
        }
    }
}

/** The big button over a paused video, or play again once it's over. */
@Composable
private fun PlayMark(visible: Boolean, ended: Boolean, modifier: Modifier = Modifier) {
    AnimatedVisibility(
        visible = visible,
        enter = scaleIn(MaterialTheme.motionScheme.fastSpatialSpec()) + fadeIn(),
        exit = scaleOut() + fadeOut(),
        modifier = modifier,
    ) {
        Box(
            Modifier.size(72.dp).clip(MaterialShapes.Cookie6Sided.toShape()).background(MaterialTheme.colorScheme.primaryContainer),
            contentAlignment = Alignment.Center,
        ) {
            Icon(if (ended) FcIcons.Replay else FcIcons.Play, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimaryContainer, modifier = Modifier.size(32.dp))
        }
    }
}

@Composable
private fun Details(clip: Clip, info: VideoInfo?, from: String?) {
    val kind = if (clip.kind == "clip") "clip" else "recording"
    Column(Modifier.padding(horizontal = 20.dp, vertical = 18.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text("${time(clip.created)}, ${day(clip.created)}", style = MaterialTheme.typography.headlineSmall)
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Icon(FcIcons.Headset, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(18.dp))
            Text(
                listOfNotNull(kind, clip.durationS?.let(::length), from?.let { "from $it" }).joinToString(" · "),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        val chips = buildList {
            info?.let {
                add("${it.width}×${it.height}")
                it.fps?.let { f -> add("$f fps") }
                it.codec?.let(::add)
                add(it.shape)
            }
            add(size(clip.size))
        }
        FlowRow(
            Modifier.padding(top = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            for (c in chips) {
                Text(
                    c,
                    style = DataStyle,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier
                        .border(BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant), RoundedCornerShape(8.dp))
                        .padding(horizontal = 12.dp, vertical = 6.dp),
                )
            }
        }
    }
}

private suspend fun probe(context: Context, uri: Uri): VideoInfo? = withContext(Dispatchers.IO) {
    runCatching {
        val r = MediaMetadataRetriever()
        try {
            r.setDataSource(context, uri)
            val w = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH)?.toIntOrNull() ?: 0
            val h = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_HEIGHT)?.toIntOrNull() ?: 0
            val frames = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_FRAME_COUNT)?.toLongOrNull()
            val ms = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull()
            val fps = if (frames != null && ms != null && ms > 0) (frames * 1000.0 / ms).roundToInt() else null
            VideoInfo(w, h, fps, codecOf(context, uri))
        } finally {
            r.release()
        }
    }.getOrNull()
}

private fun codecOf(context: Context, uri: Uri): String? = runCatching {
    val ex = MediaExtractor()
    try {
        ex.setDataSource(context, uri, null)
        val mime = (0 until ex.trackCount).mapNotNull { ex.getTrackFormat(it).getString(MediaFormat.KEY_MIME) }.firstOrNull { it.startsWith("video/") }
        when (mime) {
            MediaFormat.MIMETYPE_VIDEO_HEVC -> "hevc"
            MediaFormat.MIMETYPE_VIDEO_AVC -> "h264"
            MediaFormat.MIMETYPE_VIDEO_AV1 -> "av1"
            else -> mime?.removePrefix("video/")
        }
    } finally {
        ex.release()
    }
}.getOrNull()

private fun share(context: Context, clip: Clip) {
    val send = Intent(Intent.ACTION_SEND)
        .setType("video/mp4")
        .putExtra(Intent.EXTRA_STREAM, Uri.parse(clip.location))
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    context.startActivity(Intent.createChooser(send, if (clip.kind == "clip") "share clip" else "share recording"))
}

private fun open(context: Context, clip: Clip) {
    val view = Intent(Intent.ACTION_VIEW)
        .setDataAndType(Uri.parse(clip.location), "video/mp4")
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    runCatching { context.startActivity(view) }
}
