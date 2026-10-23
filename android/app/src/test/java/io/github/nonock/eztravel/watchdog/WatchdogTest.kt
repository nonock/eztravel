package io.github.nonock.eztravel.watchdog

import io.github.nonock.eztravel.backend.Status
import io.github.nonock.eztravel.backend.Status.Cause
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.backend.Status.Outage
import org.junit.Assert.assertEquals
import org.junit.Test
import java.time.Instant

class WatchdogTest {
    private fun at(time: String) = Instant.parse("2026-11-03T${time}Z")

    private val now = at("06:50:00")

    private fun status(watching: Direction?, outage: Outage? = null, next: Instant? = at("15:00:00")) =
        Status(watching, at("06:48:00"), next, outage, emptyList(), emptyList())

    @Test
    fun aBackendThatChecksIsFineAndLookedAtAgainDuringTheWindow() {
        assertEquals(Check(Finding.Fine, at("07:10:00")), Watchdog.check(status(Direction.OUTBOUND), now, failures = 0))
    }

    @Test
    fun outsideTheWindowsTheNextLookIsJustAfterTheNextOneOpens() {
        assertEquals(Check(Finding.Fine, at("15:05:00")), Watchdog.check(status(null), now, failures = 0))
        // No window to come: nothing to look at.
        assertEquals(Check(Finding.Fine, null), Watchdog.check(status(null, next = null), now, failures = 0))
    }

    @Test
    fun whatTheBackendCannotSayItselfIsFoundOut() {
        // Its checks stopped, or its notifications cannot be sent: it cannot tell the phone.
        for (cause in listOf(Cause.SERVER, Cause.FIREBASE)) {
            val outage = Outage(at("06:30:00"), cause)
            assertEquals(Check(Finding.Silent(outage), at("07:10:00")), Watchdog.check(status(Direction.OUTBOUND, outage), now, failures = 0))
        }
        // PRIM cannot be read: the backend notifies that itself.
        val prim = Outage(at("06:30:00"), Cause.PRIM)
        assertEquals(Finding.Fine, Watchdog.check(status(Direction.OUTBOUND, prim), now, failures = 0).finding)
    }

    @Test
    fun aBackendThatDoesNotAnswerIsTriedAgainForTwoHours() {
        assertEquals(Check(Finding.Unreachable, at("07:10:00")), Watchdog.check(null, now, failures = 0))
        assertEquals(Check(Finding.Unreachable, at("07:10:00")), Watchdog.check(null, now, failures = Watchdog.RETRIES - 2))
        // The sixth time in a row: no more until the app is opened.
        assertEquals(Check(Finding.Unreachable, null), Watchdog.check(null, now, failures = Watchdog.RETRIES - 1))
    }
}
