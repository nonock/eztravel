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
    fun aClockTimeHasNoDateEvenTomorrow() {
        assertEquals("07:42", Instant.parse("2026-11-03T06:42:00Z").clock(paris, Locale.FRANCE))
        assertEquals("00:10", Instant.parse("2026-11-03T23:10:00Z").clock(paris, Locale.FRANCE))
    }

    @Test
    fun minutesLeftAreRoundedDown() {
        val now = Instant.parse("2026-11-03T06:50:00Z")
        assertEquals(3L, Instant.parse("2026-11-03T06:53:30Z").minutesFrom(now))
        assertEquals(0L, Instant.parse("2026-11-03T06:50:59Z").minutesFrom(now))
        assertEquals(0L, Instant.parse("2026-11-03T06:49:00Z").minutesFrom(now))
    }

    @Test
    fun theDayIsTheOneOfThePhoneTimeZone() {
        // 23:30 UTC on Monday is already Tuesday in Paris.
        assertEquals("00:30", shown("2026-11-02T23:30:00Z"))
    }
}
