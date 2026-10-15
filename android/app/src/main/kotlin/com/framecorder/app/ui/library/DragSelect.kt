package com.framecorder.app.ui.library

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.withFrameMillis
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.AwaitPointerEventScope
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerInputChange
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChangedIgnoreConsumed
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.unit.dp
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/**
 * Swipe to select, the way a photos app does it: hold a clip and, without lifting, drag across others,
 * and every clip from the one held to the one under the finger is selected, in the grid's order, days
 * included. Dragging back shrinks the range to what was selected before. Holding a clip that's already
 * selected deselects the range instead. While anything's selected, a sideways move from a clip starts the
 * same without the hold. Near the top or bottom the grid scrolls by itself, faster closer to the edge, so
 * a range can run past one screen.
 *
 * A plain swipe still scrolls and a tap still opens or toggles: this only takes the pointer once a hold
 * or a sideways move says it's a selection, and leaves it alone until then.
 */
@Composable
fun Modifier.dragToSelect(
    grid: LazyGridState,
    /** The grid's own content padding, which its items' offsets don't include. */
    contentPadding: PaddingValues,
    /** The clips' keys, in the order the grid shows them. */
    order: List<String>,
    picked: Set<String>,
    selecting: Boolean,
    onPicked: (Set<String>) -> Unit,
): Modifier {
    val haptics = LocalHapticFeedback.current
    val scope = rememberCoroutineScope()
    val direction = LocalLayoutDirection.current
    val index = remember(order) { order.withIndex().associate { (i, key) -> key to i } }
    val current by rememberUpdatedState(Picking(order, index, picked, selecting, onPicked))
    return pointerInput(grid) {
        val inset = Offset(contentPadding.calculateStartPadding(direction).toPx(), contentPadding.calculateTopPadding().toPx())
        val topZone = inset.y + 96.dp.toPx()
        val bottomZone = 160.dp.toPx()
        val fastest = 1800.dp.toPx()

        /** The clip under [at] as its place in the order, or with [nearest] the closest one on screen. */
        fun tileAt(at: Offset, nearest: Boolean): Int? {
            var best: Int? = null
            var bestAway = Float.MAX_VALUE
            for (item in grid.layoutInfo.visibleItemsInfo) {
                val i = current.index[item.key as? String ?: continue] ?: continue
                val left = item.offset.x + inset.x
                val top = item.offset.y + inset.y
                val dx = max(0f, max(left - at.x, at.x - (left + item.size.width)))
                val dy = max(0f, max(top - at.y, at.y - (top + item.size.height)))
                if (dx == 0f && dy == 0f) return i
                // rows first, then across, so a finger between rows goes with the nearer row
                val away = dy * 4096 + dx
                if (nearest && away < bestAway) {
                    bestAway = away
                    best = i
                }
            }
            return best
        }

        var range: Range? = null
        var finger = Offset.Zero
        var speed = 0f
        var scrolling: Job? = null

        /** Selects or deselects everything from the held clip to [at], on top of what was picked before the drag. */
        fun extend(at: Int): Boolean {
            val r = range ?: return false
            val order = current.order
            if (order.isEmpty()) return false
            val from = min(r.anchor, at).coerceIn(order.indices)
            val to = max(r.anchor, at).coerceIn(order.indices)
            val keys = order.subList(from, to + 1)
            val next = if (r.adding) r.before + keys else r.before - keys.toSet()
            if (next == current.picked) return false
            current.onPicked(next)
            return true
        }

        fun follow() {
            val at = tileAt(finger, nearest = true) ?: return
            if (extend(at)) haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick)
        }

        /** Scrolls while the finger sits in an edge, and keeps the range following what passes under it. */
        fun drift(to: Float) {
            speed = to
            if (to == 0f) {
                scrolling?.cancel()
                scrolling = null
            } else if (scrolling == null) {
                scrolling = scope.launch {
                    var last = withFrameMillis { it }
                    while (true) {
                        withFrameMillis { now ->
                            grid.dispatchRawDelta(speed * (now - last) / 1000f)
                            last = now
                        }
                        follow()
                    }
                }
            }
        }

        fun edgeSpeed(y: Float): Float = when {
            y < topZone -> -fastest * ((topZone - y) / (topZone - inset.y)).coerceIn(0f, 1f)
            y > size.height - bottomZone -> fastest * ((y - (size.height - bottomZone)) / bottomZone).coerceIn(0f, 1f)
            else -> 0f
        }

        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
            val anchor = tileAt(down.position, nearest = false) ?: return@awaitEachGesture
            // a hold, or a sideways move while selecting, makes it a selection; a lift, a scroll or a
            // straight swipe leaves it to the tile and the grid
            val sweep = withTimeoutOrNull(viewConfiguration.longPressTimeoutMillis) { sweeps(down) { current.selecting } }
            if (sweep == false) return@awaitEachGesture

            val before = current.picked
            range = Range(anchor, before, adding = current.order[anchor] !in before)
            extend(anchor)
            haptics.performHapticFeedback(HapticFeedbackType.LongPress)
            try {
                // from here on it's ours: nothing scrolls, and the tile under the finger gets no click on the lift
                while (true) {
                    val event = awaitPointerEvent(PointerEventPass.Initial)
                    event.changes.forEach { it.consume() }
                    val change = event.changes.firstOrNull { it.id == down.id }
                    if (change == null || !change.pressed) break
                    finger = change.position
                    follow()
                    drift(edgeSpeed(finger.y))
                }
            } finally {
                drift(0f)
                range = null
            }
        }
    }
}

/** What the grid is showing and has picked, read fresh by the gesture as it goes. */
private class Picking(
    val order: List<String>,
    val index: Map<String, Int>,
    val picked: Set<String>,
    val selecting: Boolean,
    val onPicked: (Set<String>) -> Unit,
)

/** A drag in progress: from which clip, what was picked when it began, and whether it adds or removes. */
private class Range(val anchor: Int, val before: Set<String>, val adding: Boolean)

/**
 * Watches the finger after [down] until it says what the gesture is: true for a sideways move while
 * [selecting], false once it lifts, scrolls, or anything else takes it. A still finger just keeps waiting,
 * so the caller's timeout is what makes a hold.
 */
private suspend fun AwaitPointerEventScope.sweeps(down: PointerInputChange, selecting: () -> Boolean): Boolean {
    while (true) {
        val change = awaitPointerEvent(PointerEventPass.Final).changes.firstOrNull { it.id == down.id } ?: return false
        // the tile's click takes the down itself, so only a move someone took means a scroll
        if (!change.pressed || (change.isConsumed && change.positionChangedIgnoreConsumed())) return false
        val moved = change.position - down.position
        if (moved.getDistance() > viewConfiguration.touchSlop) return selecting() && abs(moved.x) > abs(moved.y)
    }
}
