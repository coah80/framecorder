package com.framecorder.app.ui.common

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.core.content.FileProvider
import com.framecorder.app.sync.MediaGallery
import com.framecorder.core.Clip

/** Hands these to another app through the share sheet, one or many at once. */
fun share(context: Context, gallery: MediaGallery, clips: List<Clip>) {
    if (clips.isEmpty()) return
    val send = sendIntent(clips.map { shareUri(context, gallery, it) to it.name }, title = if (clips.size == 1) clips.single().name else what(clips))
    context.startActivity(Intent.createChooser(send, "share ${what(clips)}"))
}

/**
 * A uri for the clip that carries its name: through our own provider when
 * the file can be read, else MediaStore's (which share sheets show as a number).
 */
fun shareUri(context: Context, gallery: MediaGallery, clip: Clip): Uri =
    gallery.file(clip.location)?.let { FileProvider.getUriForFile(context, "${context.packageName}.files", it) } ?: Uri.parse(clip.location)

/**
 * A send intent for these videos that carries their file names, so the share
 * sheet and whatever gets picked show "2026-09-26_14-00-00.mp4", not the
 * gallery's row id.
 */
fun sendIntent(videos: List<Pair<Uri, String>>, title: String): Intent {
    val (first, firstName) = videos.first()
    val send = if (videos.size == 1) {
        Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, first)
    } else {
        Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, ArrayList(videos.map { it.first }))
    }
    val clip = ClipData(firstName, arrayOf("video/mp4"), ClipData.Item(first))
    videos.drop(1).forEach { (uri, _) -> clip.addItem(ClipData.Item(uri)) }
    send.clipData = clip
    return send
        .setType("video/mp4")
        .putExtra(Intent.EXTRA_TITLE, title)
        .putExtra(Intent.EXTRA_SUBJECT, title)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
}

/** "clip", "recording", "3 clips", "3 recordings", "3 videos" when they're mixed. */
fun what(clips: List<Clip>): String {
    val n = clips.size
    val word = when {
        clips.all { it.kind == "clip" } -> "clip"
        clips.none { it.kind == "clip" } -> "recording"
        else -> "video"
    }
    return if (n == 1) word else "$n ${word}s"
}
