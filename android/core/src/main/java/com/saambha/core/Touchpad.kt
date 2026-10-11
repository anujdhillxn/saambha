package com.saambha.core

import kotlin.math.hypot

/**
 * Turns touches on the touchpad into what the [mouse] does: slide a finger to move it, tap to
 * click, hold still and let go (or tap with two fingers) to right-click, slide two fingers or
 * turn the watch's crown to scroll. Positions are in dp.
 */
class Touchpad(private val mouse: Mouse, private val settings: Settings) {

    private var fingers = 0  // 0: nothing is touching
    private var mostFingers = 0
    private var startMs = 0L
    private var lastX = 0f
    private var lastY = 0f
    private var travel = 0f  // how far the fingers slid, in dp

    // What hasn't added up to a whole pixel or notch yet.
    private var restX = 0f
    private var restY = 0f
    private var restScroll = 0f
    private var restCrown = 0f

    /** [count] fingers are down, centred on [x], [y]. */
    fun touch(count: Int, x: Float, y: Float, timeMs: Long) {
        if (fingers == 0) {
            mostFingers = 0
            startMs = timeMs
            travel = 0f
            restX = 0f
            restY = 0f
            restScroll = 0f
        } else if (count == fingers) {
            // A finger landing or lifting moves the centre without anything sliding: that's skipped.
            val dx = x - lastX
            val dy = y - lastY
            travel += hypot(dx, dy)
            if (count == 1) move(dx, dy, timeMs) else scroll(dy, timeMs)
        }
        fingers = count
        mostFingers = maxOf(mostFingers, count)
        lastX = x
        lastY = y
    }

    /** All fingers lifted. Returns the button it clicked, if it was a tap. */
    fun release(timeMs: Long): MouseButton? {
        if (fingers == 0) return null
        fingers = 0
        if (travel > settings.tapSlopDp) return null
        val held = timeMs - startMs >= settings.rightClickHoldMs
        val button = if (mostFingers > 1 || held) MouseButton.RIGHT else MouseButton.LEFT
        mouse.click(button, timeMs)
        return button
    }

    /** The touch was taken away (by the system): nothing to click. */
    fun cancel() {
        fingers = 0
    }

    /** The crown turned by [amount] (MotionEvent.AXIS_SCROLL): positive is towards the top. */
    fun turn(amount: Float, timeMs: Long) {
        restCrown += amount * settings.crownNotchesPerTurn
        val notches = restCrown.toInt()
        if (notches != 0) {
            mouse.scroll(notches, timeMs)
            restCrown -= notches
        }
    }

    private fun move(dx: Float, dy: Float, timeMs: Long) {
        restX += dx * settings.pointerSpeed
        restY += dy * settings.pointerSpeed
        val x = restX.toInt()
        val y = restY.toInt()
        if (x != 0 || y != 0) {
            mouse.move(x, y, timeMs)
            restX -= x
            restY -= y
        }
    }

    /** The page follows the fingers: sliding them up scrolls down. */
    private fun scroll(dy: Float, timeMs: Long) {
        restScroll += dy / settings.scrollStepDp
        val notches = restScroll.toInt()
        if (notches != 0) {
            mouse.scroll(notches, timeMs)
            restScroll -= notches
        }
    }
}
