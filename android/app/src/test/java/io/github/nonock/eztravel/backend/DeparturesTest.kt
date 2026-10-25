package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.backend.Departures.Departure
import io.github.nonock.eztravel.backend.Departures.Leg
import io.github.nonock.eztravel.backend.Departures.State
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
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
        assertEquals(Position(48.947, 2.248), train.fromPosition)
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
                Leg(
                    Line("L", Mode.TRAIN),
                    color = null,
                    "Gare Centrale",
                    "Les Aubépines",
                    emptyList(),
                    Duration.ofMinutes(25),
                    fromPosition = Position(48.876, 2.325),
                ),
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
    fun theTrainShownFirstIsThePickedOneOrElseTheNextToCatch() {
        val departures = Departures.parse(fixture("departures"))
        // The 8:07 is picked, 4 minutes late: leave at 7:59.
        assertEquals(
            Departures.Featured(departures.legs[0], departures.legs[0].departures!![1], at("06:59:00")),
            departures.featured(at("06:50:00")),
        )
        // Even too late to catch, a pick stays first: it says so.
        assertEquals("j2", departures.featured(at("07:05:00"))?.departure?.journey)
        // Without a pick, the next one to catch: the 7:52 until 7:40, then the 8:07.
        val none = departures.picking(null)
        assertEquals("j1", none.featured(at("06:40:00"))?.departure?.journey)
        assertEquals("j2", none.featured(at("06:41:00"))?.departure?.journey)
        // A cancelled pick gives way; the 8:22 is cancelled too.
        assertEquals("j2", departures.picking("j3").featured(at("06:41:00"))?.departure?.journey)
        assertNull(none.featured(at("07:00:00")))
        // No ride with a time to the platform, or no departure read.
        assertNull(Departures(Direction.OUTBOUND, listOf(departures.legs[1])).featured(at("06:00:00")))
        val unread = departures.copy(legs = listOf(departures.legs[0].copy(departures = null)))
        assertNull(unread.featured(at("06:00:00")))
    }

    @Test
    fun withoutAPickTheUsualTrainIsShownFirst() {
        val json = JSONObject(fixture("departures"))
        val trains = json.getJSONArray("legs").getJSONObject(0).getJSONArray("departures")
        trains.getJSONObject(1).put("picked", false).put("usual", true)
        // From a backend without usual trains.
        trains.getJSONObject(0).remove("usual")
        val departures = Departures.parse(json.toString())
        assertEquals(listOf(false, true, false), departures.legs[0].departures!!.map { it.usual })
        // The 8:07, rather than the next one to catch, the 7:52.
        val featured = departures.featured(at("06:30:00"))
        assertEquals("j2" to at("06:59:00"), featured?.departure?.journey to featured?.leave)
        // Past its time to leave: on the way to it, most likely.
        assertEquals("j2", departures.featured(at("07:05:00"))?.departure?.journey)
        // A pick comes first, and the usual train is no longer marked.
        val picked = departures.picking("j1")
        assertEquals("j1", picked.featured(at("06:30:00"))?.departure?.journey)
        assertEquals(listOf(false, false, false), picked.legs[0].departures!!.map { it.usual })
        // Dropping the pick leaves the usual train to the next read.
        assertEquals(listOf(false, true, false), departures.picking(null).legs[0].departures!!.map { it.usual })
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

    /** A train on a ride with twelve minutes to the platform, at [time] UTC. */
    private fun train(time: String, journey: String, state: State = State.ON_TIME, picked: Boolean = false, usual: Boolean = false) =
        Departure(at(time), at(time), null, null, "Gare Centrale", null, state, journey, picked, usual)

    @Test
    fun aLongListIsShortenedToTheTrainsTooSoonAndThreeToCatch() {
        val departures = listOf(
            // Too soon to leave for at 6:50, but not for whoever is on the way.
            train("06:52:00", "a"),
            train("06:58:00", "b"),
            // Leave now, then later: three can be caught by the 7:25.
            train("07:02:00", "c"),
            train("07:10:00", "d", State.CANCELLED),
            train("07:15:00", "e"),
            train("07:25:00", "f"),
            train("07:40:00", "g"),
            train("07:55:00", "h", usual = true),
            train("08:10:00", "i"),
        )
        val leg = Leg(Line("L", Mode.TRAIN), null, "Les Aubépines", "Gare Centrale", departures, Duration.ofMinutes(12))
        val now = at("06:50:00")
        assertEquals(listOf("a", "b", "c", "d", "e", "f", "h"), leg.shortList(now).map { it.journey })
        // The train picked stays in sight, wherever it is.
        val picked = leg.copy(departures = departures.map { if (it.journey == "i") it.copy(picked = true, usual = false) else it.copy(usual = false) })
        assertEquals(listOf("a", "b", "c", "d", "e", "f", "i"), picked.shortList(now).map { it.journey })
        // Fewer than three to catch: all of them.
        assertEquals(departures, leg.shortList(at("07:20:00")))
        // Without a time to the platform, the next four: no usual train there.
        val metro = leg.copy(reach = null, departures = departures.map { it.copy(usual = false) })
        assertEquals(listOf("a", "b", "c", "d"), metro.shortList(now).map { it.journey })
        // Nothing read.
        assertEquals(emptyList<Departure>(), leg.copy(departures = null).shortList(now))
    }

    @Test
    fun readsTheBackupsNeeded() {
        val departures = Departures.parse(fixture("departures-backup"))
        assertEquals(listOf(Line("L", Mode.TRAIN), Line("X", Mode.METRO)), departures.legs.map { it.line })
        val backup = departures.backups.single()
        assertEquals(listOf(Status.LineState(Line("X", Mode.METRO), Severity.BLOCKING, 0xFFE07A1F.toInt())), backup.insteadOf)
        assertEquals(
            listOf(
                Leg(
                    Line("Y", Mode.METRO),
                    0xFF3E9C6B.toInt(),
                    "Gare Centrale",
                    "Mairie",
                    listOf(
                        Departure(null, at("06:51:00"), null, null, "Porte Nord", null, State.ON_TIME, "y1"),
                        Departure(null, at("06:57:00"), null, null, "Porte Nord", null, State.ON_TIME, "y2"),
                    ),
                    fromPosition = Position(48.874, 2.327),
                ),
            ),
            backup.legs,
        )
        // A ride disrupted too.
        val json = JSONObject(fixture("departures-backup"))
        val ride = json.getJSONArray("backups").getJSONObject(0).getJSONArray("legs").getJSONObject(0)
        ride.put("severity", "disrupted")
        assertEquals(Severity.DISRUPTED, Departures.parse(json.toString()).backups[0].legs[0].severity)
        ride.put("severity", "blocking")
        assertEquals(Severity.BLOCKING, Departures.parse(json.toString()).backups[0].legs[0].severity)
    }

    @Test
    fun theRideNearestThePhoneComesFirst() {
        val departures = Departures.parse(fixture("departures"))
        fun order(here: Position?, of: Departures = departures) = of.nearestFirst(here).map { it.line.name }
        // L from Les Aubépines, then X from Gare Centrale, as the way goes.
        assertEquals(listOf("L", "X"), order(null))
        assertEquals(listOf("L", "X"), order(Position(48.94, 2.25)))
        // At Gare Centrale, the train is behind: the metro first.
        assertEquals(listOf("X", "L"), order(Position(48.8752, 2.3262)))
        // A ride whose stop is not placed yet keeps the way's order.
        val unplaced = departures.copy(legs = departures.legs.map { it.copy(fromPosition = null) })
        assertEquals(listOf("L", "X"), order(Position(48.8752, 2.3262), unplaced))
        // From the nearest on, then those behind: three rides.
        val third = departures.legs[0].copy(line = Line("Z", Mode.TRAIN), fromPosition = Position(48.95, 2.40))
        val three = departures.copy(legs = departures.legs + third)
        assertEquals(listOf("X", "Z", "L"), order(Position(48.8752, 2.3262), three))
    }

    @Test
    fun mostOfTheTimeNoBackupIsNeeded() {
        assertEquals(emptyList<Departures.Backup>(), Departures.parse(fixture("departures")).backups)
        // From a backend without them.
        val json = JSONObject(fixture("departures")).apply { remove("backups") }
        assertEquals(emptyList<Departures.Backup>(), Departures.parse(json.toString()).backups)
    }
}
