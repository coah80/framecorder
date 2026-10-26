package com.framecorder.app

import android.content.Context
import androidx.core.content.edit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/** The few things you can change in settings, kept on the phone. */
class Prefs(context: Context) {
    private val store = context.getSharedPreferences("prefs", Context.MODE_PRIVATE)

    private val _wallpaper = MutableStateFlow(store.getBoolean(WALLPAPER, false))
    /** Use the wallpaper's colors (Android 12+) instead of framecorder's. */
    val wallpaper: StateFlow<Boolean> = _wallpaper

    private val _background = MutableStateFlow(store.getBoolean(BACKGROUND, true))
    /** Keep syncing with the app closed, from a foreground service. */
    val background: StateFlow<Boolean> = _background

    private val _clipNotes = MutableStateFlow(store.getBoolean(CLIP_NOTES, true))
    /** A notification per clip that lands. The syncing one is Android's own and stays. */
    val clipNotes: StateFlow<Boolean> = _clipNotes

    private val _appUpdate = MutableStateFlow(store.getString(APP_UPDATE, null))
    /** A newer framecorder for this phone, "0.3.0", from the last check that found one. */
    val appUpdate: StateFlow<String?> = _appUpdate

    /** Whether the first-start ask (videos and notifications) has been shown. */
    val askedOnce: Boolean get() = store.getBoolean(ASKED_ONCE, false)

    /** When the app last asked GitHub about a newer version, unix ms. */
    val appUpdateCheckedAt: Long get() = store.getLong(APP_UPDATE_AT, 0)

    fun setWallpaper(on: Boolean) {
        store.edit { putBoolean(WALLPAPER, on) }
        _wallpaper.value = on
    }

    fun setBackground(on: Boolean) {
        store.edit { putBoolean(BACKGROUND, on) }
        _background.value = on
    }

    fun setClipNotes(on: Boolean) {
        store.edit { putBoolean(CLIP_NOTES, on) }
        _clipNotes.value = on
    }

    fun setAskedOnce() = store.edit { putBoolean(ASKED_ONCE, true) }

    fun setAppUpdate(newer: String?, checkedAt: Long) {
        store.edit {
            putString(APP_UPDATE, newer)
            putLong(APP_UPDATE_AT, checkedAt)
        }
        _appUpdate.value = newer
    }

    private companion object {
        const val WALLPAPER = "wallpaper_colors"
        const val BACKGROUND = "background_sync"
        const val CLIP_NOTES = "clip_notifications"
        const val ASKED_ONCE = "asked_once"
        const val APP_UPDATE = "app_update"
        const val APP_UPDATE_AT = "app_update_checked_at"
    }
}
