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

    fun setWallpaper(on: Boolean) {
        store.edit { putBoolean(WALLPAPER, on) }
        _wallpaper.value = on
    }

    fun setBackground(on: Boolean) {
        store.edit { putBoolean(BACKGROUND, on) }
        _background.value = on
    }

    private companion object {
        const val WALLPAPER = "wallpaper_colors"
        const val BACKGROUND = "background_sync"
    }
}
