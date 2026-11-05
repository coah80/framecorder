package com.framecorder.app.ui.report

import org.json.JSONObject

/** The most anyone can type into a report; the relay refuses more. */
const val DESCRIPTION_LIMIT = 4000

/**
 * The short version of a report, read from the JSON the core made, for
 * saying plainly what's about to go on the issue. Anything the core
 * couldn't fill shows as what it is, never as an error.
 */
data class ReportSummary(
    /** "framecorder android 0.2.0 · Android 16 (SDK 36) · Google Pixel 8". */
    val app: String,
    /** The headset's framecorder, SteamOS and SteamVR, or null when they couldn't be read. */
    val headset: String?,
    /** Why the headset's details are missing, in the core's words. */
    val headsetUnavailable: String?,
    val recentErrors: Int,
    /** Names of the headset logs included, newest part of each. */
    val headsetLogs: List<String>,
    /** Names of this app's logs included. */
    val appLogs: List<String>,
) {
    /** Label and value, one row each, in the order they read best. */
    val rows: List<Pair<String, String>>
        get() = listOf(
            "this app" to app,
            "your frame" to (headset ?: "not included: ${headsetUnavailable ?: "the headset's details couldn't be read"}"),
            "recent errors" to when (recentErrors) {
                0 -> "none found in the last 48 hours"
                1 -> "1 line from the last 48 hours"
                else -> "$recentErrors lines from the last 48 hours"
            },
            "logs" to (headsetLogs + appLogs).ifEmpty { listOf("none") }.joinToString(", "),
        )

    companion object {
        fun from(json: String): ReportSummary {
            val report = JSONObject(json)
            val app = report.optJSONObject("app")
            val headset = report.optJSONObject("headset")
            val framecorder = headset?.optJSONObject("framecorder")
            val os = headset?.optJSONObject("os")
            val steam = headset?.optJSONObject("steam")
            return ReportSummary(
                app = listOfNotNull(
                    listOfNotNull(app?.text("name"), app?.text("version")).joinToString(" ").ifBlank { null },
                    app?.text("os"),
                    app?.text("device"),
                ).joinToString(" · ").ifBlank { "this app" },
                headset = headset?.let {
                    listOfNotNull(
                        framecorder?.text("version")?.let { v -> "framecorder $v" + (framecorder.text("channel")?.let { c -> " ($c)" } ?: "") },
                        os?.let { o -> listOfNotNull(o.text("name"), o.text("version")).joinToString(" ").ifBlank { null } },
                        steam?.text("steamvr_version")?.let { v -> "SteamVR $v" },
                    ).joinToString(" · ").ifBlank { "its details, without versions" }
                },
                headsetUnavailable = report.text("headset_unavailable"),
                recentErrors = headset?.optJSONArray("recent_errors")?.length() ?: 0,
                headsetLogs = headset?.optJSONArray("logs").names(),
                appLogs = report.optJSONArray("app_logs").names(),
            )
        }

        /** A string field that's there and not JSON null; org.json would say "null" otherwise. */
        private fun JSONObject.text(key: String): String? =
            if (isNull(key)) null else optString(key).takeIf { it.isNotBlank() }

        private fun org.json.JSONArray?.names(): List<String> =
            this?.let { logs -> (0 until logs.length()).mapNotNull { logs.optJSONObject(it)?.text("name") } } ?: emptyList()
    }
}
