package com.framecorder.app.ui.common

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.ContextCompat

/**
 * The permission that lets the app see videos it didn't save itself: what an
 * earlier install left in Movies/framecorder, so they aren't downloaded again.
 */
val VIDEO_PERMISSION: String =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) Manifest.permission.READ_MEDIA_VIDEO else Manifest.permission.READ_EXTERNAL_STORAGE

/** The one sentence of why, wherever the videos permission is asked for. */
const val VIDEO_PERMISSION_WHY = "so framecorder can see the videos it saved before and not download them again."

private fun Context.has(permission: String) = ContextCompat.checkSelfPermission(this, permission) == PackageManager.PERMISSION_GRANTED

/** Whether the app can see other videos in the gallery, fully or the ones the user picked (Android 14+). */
fun Context.canSeeVideos(): Boolean =
    has(VIDEO_PERMISSION) ||
        (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE && has(Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED))

/** Whether it may post notifications. Before Android 13 it always may. */
fun Context.canNotify(): Boolean =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || has(Manifest.permission.POST_NOTIFICATIONS)

/** What the first-start ask should request: whichever of the two isn't granted yet. */
fun Context.firstAsk(): List<String> = buildList {
    if (!canSeeVideos()) add(VIDEO_PERMISSION)
    if (!canNotify()) add(Manifest.permission.POST_NOTIFICATIONS)
}
