package io.github.nonock.eztravel.home

import androidx.compose.ui.graphics.Color
import org.junit.Assert.assertEquals
import org.junit.Test

class LineBadgeTest {
    @Test
    fun aLineNameIsDarkOnALightLineColor() {
        assertEquals(Color.Black, contentOn(Color(0xFFE5D84A)))
        assertEquals(Color.Black, contentOn(Color(0xFF9CC9E8)))
    }

    @Test
    fun aLineNameIsWhiteOnADarkLineColor() {
        assertEquals(Color.White, contentOn(Color(0xFF0B5D3B)))
        assertEquals(Color.White, contentOn(Color(0xFF7B61C9)))
    }
}
