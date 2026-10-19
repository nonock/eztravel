package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.backend.Departures.Departure
import io.github.nonock.eztravel.backend.Departures.Leg
import io.github.nonock.eztravel.backend.Departures.State
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.io.File
import java.time.Duration
import java.time.Instant

class DeparturesTest {
    /** The answers the backend gives, shared with its own tests. */
    private fun fixture(name: String) = File("../../fixtures/api/$name.json").readText()

    private fun at(time: String) = Instant.parse("2026-11-03T${time}Z")

    @Test
    fun readsTheMorningDepartures() {
        val departures = Departures.parse(fixture("departures"))
        assertEquals(Direction.OUTBOUND, departures.direction)
        val (train, metro) = departures.legs
        assertEquals(Line("L", Mode.TRAIN), train.line)
        assertEquals(0xFF7B61C9.toInt(), train.color)
        assertEquals("Les Aubépines" to "Gare Centrale", train.from to train.to)
        assertEquals(Duration.ofMinutes(12), train.reach)
        assertEquals(
            listOf(
                Departure(at("06:52:00"), at("06:52:00"), at("07:04:00"), "PALO", "Gare Centrale", "2", State.ON_TIME),
                Departure(at("07:07:00"), at("07:11:00"), at("07:23:00"), "PILO", "Gare Centrale", "2", State.DELAYED),
                Departure(at("07:22:00"), at("07:22:00"), at("07:34:00"), "PALO", "Gare Centrale", "2", State.CANCELLED),
            ),
            train.departures,
        )
        assertEquals(Line("X", Mode.METRO), metro.line)
        assertNull(metro.reach)
        assertEquals(
            Departure(null, at("06:53:30"), null, null, "Porte Nord", null, State.ON_TIME),
            metro.departures?.first(),
        )
    }

    @Test
    fun readsLegsThatCannotBeShown() {
        val departures = Departures.parse(fixture("departures-unavailable"))
        assertEquals(Direction.INBOUND, departures.direction)
        assertEquals(
            listOf(
                Leg(Line("X", Mode.METRO), 0xFFE07A1F.toInt(), from = null, to = null, departures = null),
                Leg(Line("L", Mode.TRAIN), color = null, "Gare Centrale", "Les Aubépines", emptyList(), Duration.ofMinutes(25)),
            ),
            departures.legs,
        )
    }

    @Test
    fun theDelayIsInWholeMinutes() {
        val (train, metro) = Departures.parse(fixture("departures")).legs
        assertEquals(listOf(0L, 4L, 0L), train.departures?.map { it.delayMinutes })
        assertEquals(0L, metro.departures?.first()?.delayMinutes)
        val early = Departure(at("07:00:00"), at("06:59:00"), null, null, "Gare Centrale", null, State.ON_TIME)
        assertEquals(0L, early.delayMinutes)
        val halfAMinute = early.copy(expected = at("07:00:30"))
        assertEquals(0L, halfAMinute.delayMinutes)
    }

    @Test
    fun valuesFromANewerBackendFallBackSafely() {
        val json = JSONObject(fixture("departures"))
        json.put("direction", "sideways")
        json.getJSONArray("legs").getJSONObject(0).getJSONArray("departures").getJSONObject(0).put("status", "rerouted")
        val departures = Departures.parse(json.toString())
        assertEquals(Direction.OUTBOUND, departures.direction)
        // Unknown, it is shown by its times alone.
        assertEquals(State.ON_TIME, departures.legs[0].departures?.first()?.state)
        // An older backend says nothing of the time to the platform.
        json.getJSONArray("legs").getJSONObject(0).remove("reach")
        assertNull(Departures.parse(json.toString()).legs[0].reach)
    }
}
