package com.framecorder.app.ui.common

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

/** The design's icons: 24-unit strokes, 2 wide with round ends. `Icon` tints them like any other. */
object FcIcons {
    val Search = stroke("search", circle(11f, 11f, 6.5f), "m20 20-4.2-4.2")
    val More = icon("more", fills = listOf(circle(12f, 5.5f, 1.6f), circle(12f, 12f, 1.6f), circle(12f, 18.5f, 1.6f)))
    val Headset = stroke(
        "headset",
        "M3 9.5A2.5 2.5 0 0 1 5.5 7h13A2.5 2.5 0 0 1 21 9.5v5a2.5 2.5 0 0 1-2.5 2.5h-3.2a1.5 1.5 0 0 1-1.3-.8l-.7-1.3a1.5 1.5 0 0 0-2.6 0l-.7 1.3a1.5 1.5 0 0 1-1.3.8H5.5A2.5 2.5 0 0 1 3 14.5z",
    )
    val Library = stroke("library", rrect(4f, 4f, 7f, 7f, 2f), rrect(13f, 4f, 7f, 7f, 2f), rrect(4f, 13f, 7f, 7f, 2f), rrect(13f, 13f, 7f, 7f, 2f))
    val Settings = stroke("settings", "M4 7h10M18 7h2M4 17h2M10 17h10", circle(16f, 7f, 2f), circle(8f, 17f, 2f))
    val ChevronRight = stroke("chevron", "M9.5 6l6 6-6 6")
    val Back = stroke("back", "M19 12H5M11 6l-6 6 6 6")
    val Check = stroke("check", "M5 12.5l4.5 4.5L19 7.5")
    val Close = stroke("close", "M6 6l12 12M18 6L6 18")
    val SaveClip = icon("save clip", strokes = listOf("M4.5 12a7.5 7.5 0 1 0 2.2-5.3", "M4.5 4.5V8H8"), fills = listOf(circle(12f, 12f, 2.6f)))
    val Play = icon("play", fills = listOf("M9 6.2v11.6a.8.8 0 0 0 1.2.7l9.2-5.8a.8.8 0 0 0 0-1.4L10.2 5.5A.8.8 0 0 0 9 6.2z"))
    val Replay = icon("play again", strokes = listOf("M4.5 12a7.5 7.5 0 1 0 2.2-5.3", "M4.5 4.5V8H8"), fills = listOf("M10.5 9.4v5.2l4.2-2.6z"))
    val Pause = icon("pause", fills = listOf(rrect(6.5f, 5f, 4f, 14f, 1.4f), rrect(13.5f, 5f, 4f, 14f, 1.4f)))
    val Share = stroke("share", "M12 15V4M8 8l4-4 4 4M5 13v5a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-5")
    val Delete = stroke("delete", "M5 7h14M10 7V5h4v2M7 7l1 12a1 1 0 0 0 1 1h6a1 1 0 0 0 1-1l1-12")
    val Trim = stroke("trim", circle(6f, 7f, 2.5f), circle(6f, 17f, 2.5f), "M8.2 8.4L20 17M8.2 15.6L20 7")
    val Info = stroke("info", circle(12f, 12f, 9f), "M12 11v5M12 8h.01")
    val Down = stroke("down", "M12 5v14M6 13l6 6 6-6")
    val Qr = stroke("qr", "M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3", rrect(8f, 8f, 8f, 8f, 1.5f))
    val Keyboard = stroke("keyboard", rrect(3f, 6f, 18f, 12f, 3f), "M7 10h.01M11 10h.01M15 10h.01M8 14h8")
    val Link = stroke("link", "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1")
    val Game = stroke("game", rrect(2.5f, 7f, 19f, 11f, 5.5f), "M7 11v3M5.5 12.5h3M15.5 12h.01M18 14h.01")
    val Shield = stroke("shield", "M12 3L5 6v5c0 4.5 3 8.3 7 10 4-1.7 7-5.5 7-10V6z", "M9 12l2 2 4-4")
    val Wifi = icon("wifi", strokes = listOf("M2 9a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0"), fills = listOf(circle(12f, 19.5f, 1f)))
    val WifiOff = icon(
        "wifi off",
        strokes = listOf("M2 9a15 15 0 0 1 5-3.1M10.5 5.1A15 15 0 0 1 22 9M5 12.5a10 10 0 0 1 4-2.3M14.5 10.4a10 10 0 0 1 4.5 2.1M8.5 16a5 5 0 0 1 7 0M3 3l18 18"),
        fills = listOf(circle(12f, 19.5f, 1f)),
    )
    val Plus = stroke("plus", "M12 5v14M5 12h14")
    val Folder = stroke("folder", "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z")
    val Edit = stroke("edit", "M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16z", "M13.5 6.5l4 4")
    val Retry = stroke("retry", "M4 12a8 8 0 0 1 14-5.3L20 9M20 4v5h-5M20 12a8 8 0 0 1-14 5.3L4 15M4 20v-5h5")
    val Open = stroke("open", "M14 4h6v6M20 4l-9 9M18 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4")
    val Mic = stroke("mic", rrect(9f, 3f, 6f, 11f, 3f), "M5 11a7 7 0 0 0 14 0M12 18v3")
    val Storage = stroke("storage", "M8 3h9a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6z", "M9.5 7v3M12.5 7v3M15.5 7v3")
    val Bolt = icon("charging", fills = listOf("M13.5 2.5L5.5 13.5h5.5l-1 8 8-11h-5.5z"))
    val MicOff = stroke("mic off", "M15 10V6a3 3 0 0 0-5.7-1.3M9 9v2a3 3 0 0 0 4.6 2.5M5 11a7 7 0 0 0 11.5 5.4M19 11a7 7 0 0 1-.4 2.3M12 18v3M3 3l18 18")
    val Unlink = stroke("unpair", "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7M3 3l18 18")
}

private fun circle(cx: Float, cy: Float, r: Float) = "M${cx - r} ${cy}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0z"

private fun rrect(x: Float, y: Float, w: Float, h: Float, r: Float) =
    "M${x + r} ${y}h${w - 2 * r}a$r $r 0 0 1 $r ${r}v${h - 2 * r}a$r $r 0 0 1 ${-r} ${r}h${-(w - 2 * r)}a$r $r 0 0 1 ${-r} ${-r}v${-(h - 2 * r)}a$r $r 0 0 1 $r ${-r}z"

private fun stroke(name: String, vararg paths: String) = icon(name, strokes = paths.toList())

private fun icon(name: String, strokes: List<String> = emptyList(), fills: List<String> = emptyList()): ImageVector =
    ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply {
        for (d in strokes) {
            addPath(
                pathData = addPathNodes(d),
                stroke = SolidColor(Color.Black),
                strokeLineWidth = 2f,
                strokeLineCap = StrokeCap.Round,
                strokeLineJoin = StrokeJoin.Round,
            )
        }
        for (d in fills) addPath(pathData = addPathNodes(d), fill = SolidColor(Color.Black))
    }.build()
