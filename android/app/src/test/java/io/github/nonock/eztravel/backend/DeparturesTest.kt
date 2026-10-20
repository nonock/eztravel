package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.backend.Departures.Departure
import io.github.nonock.eztravel.backend.Departures.Leg
import io.github.nonock.eztravel.backend.Departures.State
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
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
                Departure(at("06:52:00"), at("06:52:00"), at("07:04:00"), "PALO", "Gare Centrale", "2", State.ON_TIME, "j1"),
                Departure(at("07:07:00"), at("07:11:00"), at("07:23:00"), "PILO", "Gare Centrale", "2", State.DELAYED, "j2", picked = true),
                Departure(at("07:22:00"), at("07:22:00"), at("07:34:00"), "PALO", "Gare Centrale", "2", State.CANCELLED, "j3"),
            ),
            train.departures,
        )
        assertEquals(Line("X", Mode.METRO), metro.line)
        assertNull(metro.reach)
        assertEquals(
            Departure(null, at("06:53:30"), null, null, "Porte Nord", null, State.ON_TIME, "m1"),
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
        // A train is known by its timetable's time, the metro by when it comes.
        assertEquals(at("07:07:00"), train.departures?.get(1)?.knownBy)
        assertEquals(at("06:53:30"), metro.departures?.first()?.knownBy)
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
        // An older backend says nothing of the time to the platform, nor of picking.
        val leg = json.getJSONArray("legs").getJSONObject(0)
        leg.remove("reach")
        for (departure in leg.getJSONArray("departures").objects()) {
            departure.remove("journey")
            departure.remove("picked")
        }
        val older = Departures.parse(json.toString()).legs[0]
        assertNull(older.reach)
        assertEquals(listOf(null, null, null), older.departures?.map { it.journey })
        assertEquals(listOf(false, false, false), older.departures?.map { it.picked })
    }

    @Test
    fun theRideRemindedIsTheFirstWithItsTimeToThePlatform() {
        val (train, metro) = Departures.parse(fixture("departures")).legs
        assertEquals(train, Departures.parse(fixture("departures")).reminded)
        // In the evening, the metro comes first, without a time to its platform.
        val evening = Departures.parse(fixture("departures-unavailable"))
        assertEquals(evening.legs[1], evening.reminded)
        assertNull(Departures(Direction.OUTBOUND, listOf(metro)).reminded)
    }

    @Test
    fun aTrainCanBeCaughtUntilItsTimeToLeave() {
        val train = Departures.parse(fixture("departures")).legs[0]
        val (onTime, late, cancelled) = train.departures!!
        // Twelve minutes to the platform: the 7:52 leaves at 7:52, so leave at 7:40.
        assertEquals(at("06:40:00"), train.leaveAt(onTime))
        assertEquals(at("06:59:00"), train.leaveAt(late))
        assertTrue(train.catchable(onTime, at("06:40:00")))
        assertFalse(train.catchable(onTime, at("06:40:01")))
        assertTrue(train.catchable(late, at("06:50:00")))
        assertFalse(train.catchable(cancelled, at("06:50:00")))
        // Without a time to the platform, nothing is caught on time.
        val metro = Departures.parse(fixture("departures")).legs[1]
        assertNull(metro.leaveAt(metro.departures!!.first()))
        assertFalse(metro.catchable(metro.departures!!.first(), at("06:00:00")))
    }

    @Test
    fun aPickMarksItsTrainOnly() {
        val departures = Departures.parse(fixture("departures"))
        fun picked(of: Departures) = of.legs.flatMap { it.departures.orEmpty() }.filter { it.picked }.map { it.journey }
        assertEquals(listOf("j1"), picked(departures.picking("j1")))
        assertEquals(emptyList<String>(), picked(departures.picking(null)))
        // A leg that cannot be read stays so.
        val unavailable = Departures.parse(fixture("departures-unavailable"))
        assertEquals(unavailable, unavailable.picking("j1"))
    }

    @Test
    fun readsWhyAPickIsRefused() {
        assertEquals(Refusal.UNAVAILABLE, Refusal.parse(fixture("pick-refused")))
        val reasons = mapOf(
            "too_late" to Refusal.TOO_LATE,
            "not_office_day" to Refusal.NOT_OFFICE_DAY,
            "outside_window" to Refusal.OUTSIDE_WINDOW,
            "unknown" to Refusal.UNKNOWN,
        )
        for ((name, refusal) in reasons) {
            assertEquals(refusal, Refusal.parse(JSONObject().put("refused", name).toString()))
        }
        // A reason from a newer backend, or no reason at all.
        assertNull(Refusal.parse(JSONObject().put("refused", "too_far").toString()))
        assertNull(Refusal.parse(JSONObject().put("error", "x").toString()))
        assertNull(Refusal.parse("Unprocessable Entity"))
    }
}
