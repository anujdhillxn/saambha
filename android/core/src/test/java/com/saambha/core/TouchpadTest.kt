package com.saambha.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class TouchpadTest {

    private val mouse = RecordingMouse()
    private val settings = Settings(pointerSpeed = 2f, scrollStepDp = 20f, crownNotchesPerTurn = 1f, tapSlopDp = 8f, rightClickHoldMs = 500)
    private val touchpad = Touchpad(mouse, settings)

    @Test
    fun `a tap clicks`() {
        touchpad.touch(1, 50f, 50f, 0)
        touchpad.touch(1, 51f, 50f, 80)

        assertEquals(MouseButton.LEFT, touchpad.release(100))
        assertEquals("click left", mouse.done.last())
    }

    @Test
    fun `holding still then letting go right-clicks`() {
        touchpad.touch(1, 50f, 50f, 0)

        assertEquals(MouseButton.RIGHT, touchpad.release(600))
        assertEquals(listOf("click right"), mouse.done)
    }

    @Test
    fun `a two-finger tap right-clicks`() {
        touchpad.touch(1, 50f, 50f, 0)
        touchpad.touch(2, 70f, 50f, 20)  // the centre jumps as the second finger lands
        touchpad.touch(1, 50f, 50f, 90)

        touchpad.release(100)

        assertEquals(listOf("click right"), mouse.done)
    }

    @Test
    fun `sliding moves the mouse faster than the finger`() {
        touchpad.touch(1, 50f, 50f, 0)
        touchpad.touch(1, 60f, 45f, 16)

        assertEquals(listOf("move 20 -10"), mouse.done)
    }

    @Test
    fun `slow slides add up to whole pixels`() {
        touchpad.touch(1, 50f, 50f, 0)
        repeat(4) { touchpad.touch(1, 50f + 0.2f * (it + 1), 50f, 16L * (it + 1)) }

        assertEquals(listOf("move 1 0"), mouse.done)
    }

    @Test
    fun `a slide is not a click`() {
        touchpad.touch(1, 50f, 50f, 0)
        touchpad.touch(1, 80f, 50f, 50)

        assertNull(touchpad.release(100))
        assertEquals(listOf("move 60 0"), mouse.done)
    }

    @Test
    fun `sliding two fingers up scrolls down, a notch per step`() {
        touchpad.touch(2, 50f, 100f, 0)
        touchpad.touch(2, 50f, 85f, 16)
        touchpad.touch(2, 50f, 55f, 32)

        assertNull(touchpad.release(100))
        assertEquals(listOf("scroll -2"), mouse.done)
    }

    @Test
    fun `a finger landing or lifting doesn't move the mouse`() {
        touchpad.touch(1, 50f, 50f, 0)
        touchpad.touch(2, 90f, 50f, 16)
        touchpad.touch(1, 130f, 50f, 32)
        touchpad.touch(1, 131f, 50f, 48)

        assertEquals(listOf("move 2 0"), mouse.done)
    }

    @Test
    fun `each touch starts afresh`() {
        touchpad.touch(1, 50f, 50f, 0)
        touchpad.touch(1, 50.4f, 50f, 16)
        touchpad.release(1000)  // a right click
        touchpad.touch(1, 10f, 10f, 2000)
        touchpad.touch(1, 10.4f, 10f, 2016)

        assertEquals(listOf("click right"), mouse.done)
        assertEquals(MouseButton.LEFT, touchpad.release(2100))
    }

    @Test
    fun `a touch the system took away doesn't click`() {
        touchpad.touch(1, 50f, 50f, 0)
        touchpad.cancel()

        assertNull(touchpad.release(100))
        assertEquals(emptyList<String>(), mouse.done)
    }

    @Test
    fun `turning the crown scrolls, adding up small turns`() {
        touchpad.turn(-0.6f, 0)
        touchpad.turn(-0.6f, 10)
        touchpad.turn(2f, 20)

        assertEquals(listOf("scroll -1", "scroll 1"), mouse.done)
    }

    @Test
    fun `says when each thing happened`() {
        touchpad.touch(1, 50f, 50f, 1000)
        touchpad.touch(1, 60f, 50f, 1016)
        touchpad.touch(2, 60f, 50f, 1032)
        touchpad.touch(2, 60f, 20f, 1048)
        touchpad.release(1064)
        touchpad.touch(1, 50f, 50f, 2000)
        touchpad.release(2050)
        touchpad.turn(1f, 3000)

        assertEquals(listOf("move 20 0", "scroll -1", "click left", "scroll 1"), mouse.done)
        assertEquals(listOf(1016L, 1048L, 2050L, 3000L), mouse.times)
    }
}
