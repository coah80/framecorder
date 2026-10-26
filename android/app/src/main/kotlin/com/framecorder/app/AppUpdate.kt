package com.framecorder.app

import android.util.Log
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import org.json.JSONObject

/** Where the phone's own framecorder gets installed from, and the release it's for. */
const val APP_DOWNLOAD = "https://github.com/coah80/framecorder/releases/latest/download/framecorder-android.apk"
private const val LATEST = "https://api.github.com/repos/coah80/framecorder/releases/latest"
private const val DAY_MS = 24 * 60 * 60 * 1000L

/**
 * Whether a newer framecorder for this phone is out. Asks GitHub once a day
 * at most, when the app opens; offline or rate limited, nothing changes.
 * This is the phone's app, nothing to do with the headset's update.
 */
class AppUpdate(private val prefs: Prefs, private val scope: CoroutineScope) {
    /** The newer version, "0.3.0", or null when this one is current (or nobody's asked yet). */
    val newer: StateFlow<String?> get() = prefs.appUpdate
    private val checking = AtomicBoolean(false)

    init {
        // what the last check found may be this very version, once it's installed
        prefs.appUpdate.value?.takeUnless { isNewer(it, BuildConfig.VERSION_NAME) }?.let { prefs.setAppUpdate(null, prefs.appUpdateCheckedAt) }
    }

    fun checkIfDue() {
        val now = System.currentTimeMillis()
        if (now - prefs.appUpdateCheckedAt < DAY_MS || !checking.compareAndSet(false, true)) return
        scope.launch(Dispatchers.IO) {
            try {
                val latest = latestTag() ?: return@launch
                prefs.setAppUpdate(latest.takeIf { isNewer(it, BuildConfig.VERSION_NAME) }, now)
            } finally {
                checking.set(false)
            }
        }
    }

    private fun latestTag(): String? = runCatching {
        val conn = URL(LATEST).openConnection() as HttpURLConnection
        try {
            conn.connectTimeout = 10_000
            conn.readTimeout = 10_000
            conn.setRequestProperty("Accept", "application/vnd.github+json")
            conn.setRequestProperty("User-Agent", "framecorder-android/${BuildConfig.VERSION_NAME}")
            if (conn.responseCode != HttpURLConnection.HTTP_OK) return null
            val tag = JSONObject(conn.inputStream.bufferedReader().readText()).optString("tag_name")
            tag.removePrefix("v").takeIf { parse(it) != null }
        } finally {
            conn.disconnect()
        }
    }.onFailure { Log.i("framecorder", "couldn't check for a newer app: ${it.message}") }.getOrNull()
}

/** "0.2.1" as (0, 2, 1); null for anything that isn't numbers and dots. */
fun parse(version: String): List<Int>? =
    version.substringBefore('-').split('.').map { it.toIntOrNull() ?: return null }.takeIf { it.isNotEmpty() }

fun isNewer(candidate: String, installed: String): Boolean {
    val a = parse(candidate) ?: return false
    val b = parse(installed) ?: return false
    for (i in 0 until maxOf(a.size, b.size)) {
        val x = a.getOrElse(i) { 0 }
        val y = b.getOrElse(i) { 0 }
        if (x != y) return x > y
    }
    return false
}
