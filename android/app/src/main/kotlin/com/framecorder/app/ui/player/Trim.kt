package com.framecorder.app.ui.player

import android.content.Context
import android.content.Intent
import android.media.MediaMetadataRetriever
import android.net.Uri
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.horizontalDrag
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.systemGestureExclusion
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.media3.common.MediaItem
import androidx.media3.transformer.Composition
import androidx.media3.transformer.ExportException
import androidx.media3.transformer.ExportResult
import androidx.media3.transformer.ProgressHolder
import androidx.media3.transformer.Transformer
import com.framecorder.app.ui.common.length
import com.framecorder.app.ui.theme.DataStyle
import java.io.File
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext

/** The shortest a trim can be. */
const val MIN_TRIM_MS = 1000L

/** "14 s" for short ones, "1:14" past a minute. */
fun span(ms: Long): String = if (ms < 60_000) "${(ms + 500) / 1000} s" else length(ms / 1000.0)

/**
 * Cuts [startMs] to [endMs] out of the video into the cache, for sharing.
 * Only the cut's first moments get re-encoded; the rest is copied as is.
 */
suspend fun exportTrim(context: Context, uri: Uri, name: String, startMs: Long, endMs: Long, onProgress: (Float) -> Unit): File =
    withContext(Dispatchers.Main) {
        val dir = File(context.cacheDir, "trims").apply { mkdirs() }
        val out = File(dir, "${name.removeSuffix(".mp4")}-trim-${startMs / 100}-${endMs / 100}.mp4")
        if (out.length() > 0) return@withContext out
        // only the latest one is kept around
        dir.listFiles()?.forEach { it.delete() }

        val item = MediaItem.Builder()
            .setUri(uri)
            .setClippingConfiguration(MediaItem.ClippingConfiguration.Builder().setStartPositionMs(startMs).setEndPositionMs(endMs).build())
            .build()
        val transformer = Transformer.Builder(context).experimentalSetTrimOptimizationEnabled(true).build()
        coroutineScope {
            val watch = launch {
                val holder = ProgressHolder()
                while (isActive) {
                    if (transformer.getProgress(holder) == Transformer.PROGRESS_STATE_AVAILABLE) onProgress(holder.progress / 100f)
                    delay(150)
                }
            }
            try {
                suspendCancellableCoroutine<Unit> { done ->
                    transformer.addListener(object : Transformer.Listener {
                        override fun onCompleted(composition: Composition, exportResult: ExportResult) = done.resume(Unit)
                        override fun onError(composition: Composition, exportResult: ExportResult, exportException: ExportException) =
                            done.resumeWithException(exportException)
                    })
                    transformer.start(item, out.path)
                    done.invokeOnCancellation { transformer.cancel() }
                }
            } catch (e: Throwable) {
                out.delete()
                throw e
            } finally {
                watch.cancel()
            }
        }
        out
    }

fun shareTrim(context: Context, file: File, recording: Boolean) {
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
    val send = Intent(Intent.ACTION_SEND)
        .setType("video/mp4")
        .putExtra(Intent.EXTRA_STREAM, uri)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    context.startActivity(Intent.createChooser(send, if (recording) "share the trimmed recording" else "share the trimmed clip"))
}

/** A row of frames from across the video, for the trim strip. Call off the main thread. */
fun filmstrip(context: Context, uri: Uri, durationMs: Long, count: Int = 8): List<ImageBitmap> = runCatching {
    val r = MediaMetadataRetriever()
    try {
        r.setDataSource(context, uri)
        (0 until count).mapNotNull { i ->
            val at = (durationMs * (i + 0.5) / count * 1000).toLong()
            r.getScaledFrameAtTime(at, MediaMetadataRetriever.OPTION_CLOSEST_SYNC, 160, 160)?.asImageBitmap()
        }
    } finally {
        r.release()
    }
}.getOrDefault(emptyList())

/**
 * The part of the video to keep, picked with a handle at each end. [onMove]
 * says where the handle being dragged is, so the player can show that frame.
 */
@Composable
fun TrimStrip(
    frames: List<ImageBitmap>,
    durationMs: Long,
    start: Long,
    end: Long,
    onChange: (start: Long, end: Long) -> Unit,
    onMove: (at: Long) -> Unit,
    onDone: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val scheme = MaterialTheme.colorScheme
    val density = LocalDensity.current
    // the drag outlives recompositions, so it reads these fresh
    val range by rememberUpdatedState(start to end)
    val change by rememberUpdatedState(onChange)
    val move by rememberUpdatedState(onMove)
    val done by rememberUpdatedState(onDone)
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(Modifier.padding(horizontal = 4.dp), verticalAlignment = Alignment.Bottom) {
            Text("trim", style = MaterialTheme.typography.titleSmall)
            Spacer(Modifier.weight(1f))
            Text(
                "${length(start / 1000.0)} to ${length(end / 1000.0)} · ${span(end - start)}",
                style = DataStyle,
                color = scheme.onSurfaceVariant,
            )
        }
        BoxWithConstraints(
            Modifier
                .fillMaxWidth()
                .height(56.dp)
                // the handles sit near the screen's edges, where a drag would otherwise mean "back"
                .systemGestureExclusion()
                .semantics { contentDescription = "trim, from ${length(start / 1000.0)} to ${length(end / 1000.0)}" },
        ) {
            val width = constraints.maxWidth.toFloat()
            fun x(ms: Long) = if (durationMs > 0) ms.toFloat() / durationMs * width else 0f
            fun ms(x: Float) = (x / width * durationMs).toLong().coerceIn(0, durationMs)

            Row(Modifier.fillMaxSize().clip(RoundedCornerShape(12.dp)).background(scheme.surfaceContainerHighest)) {
                frames.forEach { Image(it, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.weight(1f).fillMaxHeight()) }
            }
            val left = with(density) { x(start).toDp() }
            val right = with(density) { (width - x(end)).toDp() }
            // what gets left out is dimmed
            Box(Modifier.align(Alignment.CenterStart).width(left).fillMaxHeight().clip(RoundedCornerShape(topStart = 12.dp, bottomStart = 12.dp)).background(Shade))
            Box(Modifier.align(Alignment.CenterEnd).width(right).fillMaxHeight().clip(RoundedCornerShape(topEnd = 12.dp, bottomEnd = 12.dp)).background(Shade))
            Box(
                Modifier
                    .offset { IntOffset(x(start).roundToInt(), 0) }
                    .width(with(density) { (x(end) - x(start)).toDp() })
                    .fillMaxHeight()
                    .border(3.dp, scheme.primary, RoundedCornerShape(12.dp)),
            )
            Handle(Modifier.align(Alignment.CenterStart).offset { IntOffset(x(start).roundToInt() - 6.dp.roundToPx(), 0) })
            Handle(Modifier.align(Alignment.CenterStart).offset { IntOffset(x(end).roundToInt() - 6.dp.roundToPx(), 0) })

            // grabs whichever end is closer to the finger
            Box(
                Modifier.fillMaxSize().pointerInput(durationMs, width) {
                    awaitEachGesture {
                        val down = awaitFirstDown()
                        var (s, e) = range
                        val moveStart = abs(down.position.x - x(s)) <= abs(down.position.x - x(e))
                        horizontalDrag(down.id) { drag ->
                            drag.consume()
                            val at = ms(drag.position.x)
                            if (moveStart) s = at.coerceAtMost(e - MIN_TRIM_MS).coerceAtLeast(0) else e = at.coerceAtLeast(s + MIN_TRIM_MS).coerceAtMost(durationMs)
                            change(s, e)
                            move(if (moveStart) s else e)
                        }
                        done()
                    }
                },
            )
        }
    }
}

private val Shade = Color(0x9E0A080E)

@Composable
private fun Handle(modifier: Modifier) {
    Box(modifier.size(width = 12.dp, height = 32.dp).clip(RoundedCornerShape(6.dp)).background(MaterialTheme.colorScheme.primary))
}
