package com.framecorder.app.ui.common

import android.content.Context
import android.content.Intent
import android.net.Uri
import com.framecorder.core.Clip

/** Hands these to another app through the share sheet, one or many at once. */
fun share(context: Context, clips: List<Clip>) {
    if (clips.isEmpty()) return
    val uris = clips.map { Uri.parse(it.location) }
    val send = if (uris.size == 1) {
        Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, uris.single())
    } else {
        Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, ArrayList(uris))
    }
    send.setType("video/mp4").addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    context.startActivity(Intent.createChooser(send, "share ${what(clips)}"))
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
