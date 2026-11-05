package com.framecorder.app.ui.report

import org.junit.Assert.assertEquals
import org.junit.Test

class ReportSummaryTest {
    @Test
    fun saysWhatTheHeadsetPartCarries() {
        val s = ReportSummary.from(
            """
            {"version":1,"source":"android","description":"it tilts",
             "app":{"name":"framecorder android","version":"0.2.0","os":"Android 16 (SDK 36)","device":"Google Pixel 8","arch":"arm64-v8a"},
             "headset":{"framecorder":{"version":"0.2.0","channel":"dev"},
                        "steam":{"name":"coah","steamvr_version":"2.14.3","steamvr_branch":null},
                        "os":{"name":"SteamOS","version":"3.7.14"},
                        "recent_errors":["a","b","c"],
                        "logs":[{"name":"recorder.log","text":""},{"name":"vrcompositor.txt","text":""}]},
             "headset_unavailable":null,
             "app_logs":[{"name":"framecorder-android.log","text":""}]}
            """,
        )
        assertEquals("framecorder android 0.2.0 · Android 16 (SDK 36) · Google Pixel 8", s.app)
        assertEquals("framecorder 0.2.0 (dev) · SteamOS 3.7.14 · SteamVR 2.14.3", s.headset)
        assertEquals(3, s.recentErrors)
        assertEquals(listOf("recorder.log", "vrcompositor.txt"), s.headsetLogs)
        assertEquals(listOf("framecorder-android.log"), s.appLogs)
        assertEquals("3 lines from the last 48 hours", s.rows[2].second)
        assertEquals("recorder.log, vrcompositor.txt, framecorder-android.log", s.rows[3].second)
    }

    @Test
    fun saysWhyTheHeadsetIsMissingInsteadOfNull() {
        val s = ReportSummary.from(
            """{"version":1,"source":"android","description":"x","app":{"name":"framecorder android","version":"0.2.0","os":"Android 16 (SDK 36)","device":"Google Pixel 8","arch":"arm64-v8a"},
                "headset":null,"headset_unavailable":"the headset runs an older framecorder","app_logs":[]}""",
        )
        assertEquals(null, s.headset)
        assertEquals("not included: the headset runs an older framecorder", s.rows[1].second)
        assertEquals("none found in the last 48 hours", s.rows[2].second)
        assertEquals("none", s.rows[3].second)
    }

    @Test
    fun missingVersionsNeverReadAsTheWordNull() {
        val s = ReportSummary.from(
            """{"version":1,"source":"android","description":"x","app":null,
                "headset":{"framecorder":{"version":null,"channel":null},"steam":{"name":null,"steamvr_version":null},"os":{"name":null,"version":null}},
                "headset_unavailable":null,"app_logs":[]}""",
        )
        assertEquals("this app", s.app)
        assertEquals("its details, without versions", s.headset)
    }
}
