package com.framecorder.app.ui.common

import com.framecorder.core.Clip
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import kotlin.math.roundToLong

/** Sizes like people say them: 151 MB, 3.8 GB. Same rules as the Rust side. */
fun size(bytes: Long): String {
    var n = bytes.toDouble()
    var unit = 0
    while (n >= 1000 && unit < 4) {
        n /= 1000
        unit++
    }
    val digits = if (n < 10 && unit > 0) 1 else 0
    return "%.${digits}f %s".format(Locale.US, n, listOf("B", "KB", "MB", "GB", "TB")[unit])
}

/** 0:30, 12:34, 1:02:03 */
fun length(seconds: Double): String {
    val s = seconds.roundToLong().coerceAtLeast(0)
    val (h, m, sec) = Triple(s / 3600, s / 60 % 60, s % 60)
    return if (h > 0) "%d:%02d:%02d".format(h, m, sec) else "%d:%02d".format(m, sec)
}

fun rate(bytesPerSecond: Double): String = "${size(bytesPerSecond.toLong())}/s"

private val clock = DateTimeFormatter.ofPattern("h:mm a", Locale.US)
private val dayName = DateTimeFormatter.ofPattern("EEE, MMM d", Locale.US)

private fun local(unix: Long) = Instant.ofEpochSecond(unix).atZone(ZoneId.systemDefault())

/** "4:03 am" */
fun time(unix: Long): String = clock.format(local(unix)).lowercase(Locale.US)

/** "today", "yesterday", "mon, sep 28" */
fun day(unix: Long, today: LocalDate = LocalDate.now()): String = day(local(unix).toLocalDate(), today)

fun day(date: LocalDate, today: LocalDate = LocalDate.now()): String {
    return when (date) {
        today -> "today"
        today.minusDays(1) -> "yesterday"
        else -> dayName.format(date).lowercase(Locale.US)
    }
}

fun dayKey(unix: Long): LocalDate = local(unix).toLocalDate()

private val shortDayName = DateTimeFormatter.ofPattern("MMM d", Locale.US)

/** "today", "yesterday", "sep 28", for where there's little room. */
fun shortDay(date: LocalDate, today: LocalDate = LocalDate.now()): String = when (date) {
    today -> "today"
    today.minusDays(1) -> "yesterday"
    else -> shortDayName.format(date).lowercase(Locale.US)
}

/** When the newest clip from this frame landed, if anything has. */
fun lastSync(clips: List<Clip>, fingerprint: String): Long? =
    clips.filter { it.host == fingerprint }.maxOfOrNull { it.syncedAt }

/** "just now", "12 min ago", "3 h ago", then the day. */
fun ago(unix: Long, now: Long = System.currentTimeMillis() / 1000): String {
    val s = (now - unix).coerceAtLeast(0)
    return when {
        s < 60 -> "just now"
        s < 3600 -> "${s / 60} min ago"
        dayKey(unix) == LocalDate.now() -> "${s / 3600} h ago"
        else -> day(unix)
    }
}

/** The first 8 hex of a fingerprint as "3F2A · 91C0", for comparing by eye. */
fun shortPrint(fingerprint: String): String =
    fingerprint.take(8).uppercase(Locale.US).let { "${it.take(4)} · ${it.drop(4)}" }

/** "192.168.1.42:38619" -> "192.168.1.42", the port's always the same. */
fun host(addr: String): String = addr.removeSuffix(":38619")
