package io.github.nonock.eztravel.ui.theme

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ThemeTest {
    /** WCAG contrast ratio: 4.5 at least for text. */
    private fun contrast(text: Color, ground: Color): Float {
        val (light, dark) = listOf(text.luminance(), ground.luminance()).sortedDescending()
        return (light + 0.05f) / (dark + 0.05f)
    }

    @Test
    fun theAppIsTheLogosNavyWithItsAmber() {
        assertEquals(Navy, EztravelColors.background)
        assertEquals(Navy, EztravelColors.surface)
        assertEquals(Amber, EztravelColors.primary)
    }

    @Test
    fun textIsReadableOnEveryGroundItIsDrawnOn() {
        with(EztravelColors) {
            for ((name, text, ground) in listOf(
                Triple("body", onBackground, background),
                Triple("secondary text", onSurfaceVariant, surface),
                Triple("button", onPrimary, primary),
                Triple("amber text", primary, background),
                Triple("outage", error, background),
                Triple("all fine", Good, surfaceContainerLow),
                Triple("next train", onSurface, surfaceContainerHigh),
                Triple("countdown on the next train", primary, surfaceContainerHigh),
                Triple("date picker", onSurface, surfaceContainerHigh),
            )) {
                assertTrue("$name: ${contrast(text, ground)}", contrast(text, ground) >= 4.5f)
            }
        }
    }

    @Test
    fun eachDayOfTheCalendarIsReadableAndStandsOutFromTheGround() {
        for ((name, day) in listOf("office" to OfficeDay, "remote" to RemoteDay, "away" to AwayDay)) {
            // Its date, in navy.
            assertTrue("$name date: ${contrast(Navy, day)}", contrast(Navy, day) >= 4.5f)
            // The day itself, 3 at least for a shape that carries meaning.
            assertTrue("$name day: ${contrast(day, EztravelColors.background)}", contrast(day, EztravelColors.background) >= 3f)
        }
    }
}
