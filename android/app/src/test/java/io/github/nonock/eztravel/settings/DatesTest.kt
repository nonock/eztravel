package io.github.nonock.eztravel.settings

import org.junit.Assert.assertEquals
import org.junit.Test
import java.time.Instant
import java.time.LocalDate

class DatesTest {
    private val christmasEve = LocalDate.of(2026, 12, 24)

    @Test
    fun aDayIsItsMidnightInUtc() {
        // The Material date picker counts in UTC milliseconds.
        assertEquals(Instant.parse("2026-12-24T00:00:00Z").toEpochMilli(), christmasEve.toUtcMillis())
    }

    @Test
    fun millisecondsAreReadInUtcWhateverThePhoneTimeZone() {
        // 23:30 UTC is already the next day in Paris: the picker still means the 24th.
        assertEquals(christmasEve, Instant.parse("2026-12-24T23:30:00Z").toEpochMilli().toUtcDate())
        assertEquals(christmasEve, christmasEve.toUtcMillis().toUtcDate())
    }
}
