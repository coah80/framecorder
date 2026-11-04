package com.framecorder.app.ui.frame

import org.junit.Assert.assertEquals
import org.junit.Test

class EyeChoicesTest {
    @Test
    fun theBlendIsOnlyOfferedToAHeadsetThatCanDoIt() {
        assertEquals(listOf("left", "right"), eyeChoices(blend = false).map { it.first })
        assertEquals(listOf("left", "right", "both"), eyeChoices(blend = true).map { it.first })
        assertEquals("both eyes blended", eyeChoices(blend = true).last().second)
    }
}
