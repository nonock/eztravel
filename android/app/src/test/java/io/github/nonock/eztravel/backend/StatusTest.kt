package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.backend.Status.Cause
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.backend.Status.LineState
import io.github.nonock.eztravel.backend.Status.Outage
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.io.File
import java.time.Instant

class StatusTest {
    /** The answers the backend gives, shared with its own tests. */
    private fun fixture(name: String) = File("../../fixtures/api/$name.json").readText()

    private fun at(instant: String) = Instant.parse(instant)

    private val train = LineState(Line("L", Mode.TRAIN), null)
    private val metro = LineState(Line("X", Mode.METRO), null)
    private val now = at("2026-11-03T06:45:00Z")

    private fun watching(vararg lines: LineState, checkedAt: Instant? = now.minusSeconds(60), outage: Outage? = null) =
        Status(Direction.OUTBOUND, checkedAt, null, outage, lines.toList(), emptyList())

    @Test
    fun fineOnlyWhenWatchedJustCheckedAndEveryLineRuns() {
        assertEquals(Status.Headline.Fine, watching(train, metro).headline(now))
        // Not checked in this window yet, or a while ago: nothing to vouch for.
        assertEquals(Status.Headline.Checking, watching(train, checkedAt = null).headline(now))
        assertEquals(Status.Headline.Checking, watching(train, checkedAt = now.minusSeconds(600)).headline(now))
        // Outside the windows.
        assertEquals(Status.Headline.Idle, Status(null, now, null, null, emptyList(), emptyList()).headline(now))
    }

    @Test
    fun troubleNamesTheWorstLineAndAnOutageComesFirst() {
        val slowed = metro.copy(severity = Severity.DISRUPTED)
        val stopped = train.copy(severity = Severity.BLOCKING)
        assertEquals(Status.Headline.Trouble(slowed), watching(train, slowed).headline(now))
        assertEquals(Status.Headline.Trouble(stopped), watching(slowed, stopped).headline(now))
        val outage = Outage(now.minusSeconds(720), Cause.PRIM)
        assertEquals(Status.Headline.Out(outage), watching(stopped, outage = outage).headline(now))
        // Even outside the windows: the server stopped checking.
        assertEquals(Status.Headline.Out(outage), Status(null, null, null, outage, emptyList(), emptyList()).headline(now))
    }

    @Test
    fun readsTheStatusWhileWatching() {
        assertEquals(
            Status(
                watching = Direction.OUTBOUND,
                checkedAt = at("2026-11-03T06:42:00Z"),
                next = at("2026-11-03T15:00:00Z"),
                outage = null,
                lines = listOf(
                    LineState(Line("L", Mode.TRAIN), Severity.BLOCKING, 0xFF7B61C9.toInt()),
                    LineState(Line("X", Mode.METRO), null, 0xFFE07A1F.toInt()),
                ),
                planned = listOf("Ligne L : travaux entre Nanterre et Cergy"),
            ),
            Status.parse(fixture("status-watching")),
        )
    }

    @Test
    fun readsAnOutage() {
        val status = Status.parse(fixture("status-outage"))
        assertEquals(Direction.INBOUND, status.watching)
        assertEquals(Outage(at("2026-11-03T15:20:00Z"), Cause.PRIM), status.outage)
        assertEquals(
            listOf(
                LineState(Line("X", Mode.METRO), null, color = null),
                LineState(Line("L", Mode.TRAIN), Severity.DISRUPTED, 0xFF7B61C9.toInt()),
            ),
            status.lines,
        )
        assertEquals(emptyList<String>(), status.planned)
    }

    @Test
    fun readsTheStatusOutsideTheWindows() {
        val status = Status.parse(fixture("status-idle"))
        assertNull(status.watching)
        assertEquals(at("2026-11-03T07:14:00Z"), status.checkedAt)
        assertEquals(at("2026-11-03T15:00:00Z"), status.next)
        assertEquals(emptyList<LineState>(), status.lines)
    }

    private fun withOutage(cause: String) = JSONObject(fixture("status-outage"))
        .put("outage", JSONObject().put("since", "2026-11-03T15:20:00Z").put("cause", cause))
        .toString()

    @Test
    fun eachCauseOfAnOutageIsRead() {
        assertEquals(Cause.FIREBASE, Status.parse(withOutage("firebase")).outage?.cause)
        assertEquals(Cause.SERVER, Status.parse(withOutage("server")).outage?.cause)
    }

    @Test
    fun valuesFromANewerBackendFallBackSafely() {
        // An unknown cause still means an outage; an unknown severity still means trouble.
        assertEquals(Cause.SERVER, Status.parse(withOutage("something-new")).outage?.cause)
        val json = JSONObject(fixture("status-watching"))
        json.put("watching", "sideways").put("checked_at", JSONObject.NULL).put("next", JSONObject.NULL)
        json.getJSONArray("lines").getJSONObject(0).put("severity", "catastrophic").put("mode", "bus").put("color", "purple")
        val status = Status.parse(json.toString())
        assertNull(status.watching)
        assertNull(status.checkedAt)
        assertNull(status.next)
        assertEquals(LineState(Line("L", Mode.OTHER), Severity.DISRUPTED, color = null), status.lines[0])
        // A line without a color at all reads as one without a configured color.
        json.getJSONArray("lines").getJSONObject(0).remove("color")
        assertNull(Status.parse(json.toString()).lines[0].color)
    }
}
