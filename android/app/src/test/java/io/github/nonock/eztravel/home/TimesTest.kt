package io.github.nonock.eztravel.home

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.util.Locale

class TimesTest {
    private val paris = ZoneId.of("Europe/Paris")
    private val tuesday = LocalDate.of(2026, 11, 3)

    private fun shown(instant: String) = Instant.parse(instant).shown(tuesday, paris, Locale.FRANCE)

    @Test
    fun aTimeTodayIsJustTheTime() {
        assertEquals("07:42", shown("2026-11-03T06:42:00Z"))
    }

    @Test
    fun anotherDayComesWithItsDate() {
        val thursday = shown("2026-11-05T05:45:00Z")
        assertTrue(thursday, thursday.startsWith("jeudi 5 novembre 2026") && thursday.endsWith("06:45"))
    }

    @Test
    fun theDayIsTheOneOfThePhoneTimeZone() {
        // 23:30 UTC on Monday is already Tuesday in Paris.
        assertEquals("00:30", shown("2026-11-02T23:30:00Z"))
    }
}
