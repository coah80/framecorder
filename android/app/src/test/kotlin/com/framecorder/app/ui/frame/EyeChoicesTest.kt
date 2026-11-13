package com.framecorder.app.ui.frame

import org.junit.Assert.assertEquals
import org.junit.Test

class EyeChoicesTest {
    @Test
    fun theBlendIsOnlyOfferedToAHeadsetThatCanDoIt() {
        assertEquals(listOf("left", "right"), eyeChoices(blend = false, dominant = false).map { it.first })
        assertEquals(listOf("left", "right", "both"), eyeChoices(blend = true, dominant = false).map { it.first })
        assertEquals("both eyes blended", eyeChoices(blend = true, dominant = false).last().second)
    }

    @Test
    fun aHeadsetThatPicksTheDominantEyeGetsBothBlends() {
        val keys = eyeChoices(blend = true, dominant = true).map { it.first }
        assertEquals(listOf("left", "right", "both:left", "both:right"), keys)
        assertEquals("both:left", eyeKey("both", "left"))
        assertEquals("right", eyeKey("right", "left"))
        assertEquals("both", eyeKey("both", null))
        val patch = eyePatch("both:left")
        assertEquals("both" to "left", patch.eye to patch.dominant)
        assertEquals(null, eyePatch("right").dominant)
    }
}
