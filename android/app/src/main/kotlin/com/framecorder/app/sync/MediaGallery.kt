package com.framecorder.app.sync

import android.app.PendingIntent
import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.graphics.Bitmap
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.util.LruCache
import android.util.Size
import com.framecorder.core.CoreException
import com.framecorder.core.Gallery
import java.io.File
import java.io.FileInputStream

/**
 * Where finished downloads go: MediaStore, under Movies/framecorder (clips
 * in /clips), so they show up in the phone's gallery like any video.
 */
class MediaGallery(private val context: Context) : Gallery {
    private val resolver = context.contentResolver
    private val thumbs = object : LruCache<String, Bitmap>(24 * 1024 * 1024) {
        override fun sizeOf(key: String, value: Bitmap) = value.byteCount
    }

    override fun completed(name: String, size: ULong, subdir: String): String? {
        try {
            val collection = MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
            val selection = "${MediaStore.Video.Media.DISPLAY_NAME} = ? AND ${MediaStore.Video.Media.SIZE} = ? AND " +
                "${MediaStore.Video.Media.RELATIVE_PATH} = ? AND ${MediaStore.Video.Media.IS_PENDING} = 0"
            val args = arrayOf(name, size.toString(), folder(subdir))
            val cursor = resolver.query(collection, arrayOf(MediaStore.Video.Media._ID), selection, args, null)
                ?: error("MediaStore couldn't check the saved videos")
            return cursor.use {
                if (it.moveToFirst()) ContentUris.withAppendedId(collection, it.getLong(0)).toString() else null
            }
        } catch (e: Exception) {
            throw CoreException.Failed("couldn't check the saved videos: ${e.message}")
        }
    }

    override fun save(path: String, name: String, subdir: String): String {
        try {
            return copyIn(File(path), name, subdir).toString()
        } catch (e: Exception) {
            throw CoreException.Failed("couldn't save it to the gallery: ${e.message}")
        }
    }

    private fun folder(subdir: String): String =
        listOf(Environment.DIRECTORY_MOVIES, "framecorder", subdir).filter { it.isNotEmpty() }.joinToString("/", postfix = "/")

    private fun copyIn(source: File, name: String, subdir: String): Uri {
        val values = ContentValues().apply {
            put(MediaStore.Video.Media.DISPLAY_NAME, name)
            put(MediaStore.Video.Media.MIME_TYPE, "video/mp4")
            put(MediaStore.Video.Media.RELATIVE_PATH, folder(subdir))
            put(MediaStore.Video.Media.IS_PENDING, 1)
        }
        val collection = MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val uri = resolver.insert(collection, values) ?: error("MediaStore said no")
        try {
            resolver.openOutputStream(uri)?.use { out ->
                FileInputStream(source).use { it.copyTo(out, 1 shl 20) }
            } ?: error("couldn't open the new file")
            values.clear()
            values.put(MediaStore.Video.Media.IS_PENDING, 0)
            resolver.update(uri, values, null, null)
        } catch (e: Exception) {
            resolver.delete(uri, null, null)
            throw e
        }
        source.delete()
        return uri
    }

    /** Whether the file's still there (it may have been deleted in the gallery app). */
    fun exists(location: String): Boolean = runCatching {
        resolver.query(Uri.parse(location), arrayOf(MediaStore.Video.Media._ID), null, null, null)?.use { it.count > 0 } ?: false
    }.getOrDefault(false)

    /** A frame of the video, made by the phone. Null if it can't be read. Call off the main thread. */
    fun thumbnail(location: String, width: Int = 512): Bitmap? {
        thumbs.get(location)?.let { return it }
        return runCatching { resolver.loadThumbnail(Uri.parse(location), Size(width, width * 9 / 16), null) }
            .getOrNull()
            ?.also { thumbs.put(location, it) }
    }

    /** The thumbnail if it's already been made, without waiting for it. */
    fun cachedThumbnail(location: String): Bitmap? = thumbs.get(location)

    /**
     * Asks to delete these from the phone. Android 11+ shows its own
     * confirmation, through the returned intent; on 10 it just happens.
     */
    fun deleteRequest(locations: List<String>): PendingIntent? {
        val uris = locations.map(Uri::parse)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return MediaStore.createDeleteRequest(resolver, uris)
        }
        for (u in uris) runCatching { resolver.delete(u, null, null) }
        return null
    }
}
