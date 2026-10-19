package io.github.nonock.eztravel.notifications

import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.io.File
import java.time.Instant

class PushMessageTest {
    /** The window end in the fixtures: 8:15 in Paris on Tuesday 3 November 2026. */
    private val windowEnd = Instant.parse("2026-11-03T07:15:00Z")

    /** The payloads the backend sends, shared with its own tests. */
    private fun fixture(name: String): Map<String, String> {
        val json = JSONObject(File("../../fixtures/push/$name.json").readText())
        return json.keys().asSequence().associateWith { json.getString(it) }
    }

    @Test
    fun readsAStart() {
        assertEquals(
            PushMessage.Started(
                Line("L", Mode.TRAIN),
                Severity.BLOCKING,
                title = "Ligne L : Nanterre <-> Cergy trafic interrompu",
                text = "Le trafic est interrompu entre Nanterre et Cergy jusqu'à 20h.\nMotif : panne d'un train.",
                until = windowEnd,
            ),
            PushMessage.parse(fixture("started")),
        )
    }

    @Test
    fun readsAChange() {
        assertEquals(
            PushMessage.Changed(
                Line("X", Mode.METRO),
                Severity.DISRUPTED,
                previous = Severity.BLOCKING,
                title = "Métro X : Reprise progressive / trafic reste perturbé",
                text = "Le trafic reprend mais reste perturbé sur toute la ligne.",
                until = windowEnd,
            ),
            PushMessage.parse(fixture("changed")),
        )
    }

    @Test
    fun readsAnEnd() {
        assertEquals(PushMessage.Ended(Line("L", Mode.TRAIN), until = windowEnd), PushMessage.parse(fixture("ended")))
    }

    @Test
    fun readsPlannedDisruptions() {
        assertEquals(
            PushMessage.Planned(2, "Ligne L : offre adaptée\nMétro X : station fermée"),
            PushMessage.parse(fixture("planned")),
        )
    }

    @Test
    fun messagesAboutTheDayOrTheWatchDoNotEndWithTheWindow() {
        assertNull(PushMessage.parse(fixture("planned"))!!.until)
        assertNull(PushMessage.Outage.until)
        assertNull(PushMessage.Recovered.until)
    }

    @Test
    fun anUnreadableWindowEndKeepsTheMessage() {
        // Better shown too long than not at all.
        val message = PushMessage.parse(fixture("started") + ("until" to "soon"))
        assertEquals(Severity.BLOCKING, (message as PushMessage.Started).severity)
        assertNull(message.until)
    }

    @Test
    fun readsAnOutageAndItsEnd() {
        assertEquals(PushMessage.Outage, PushMessage.parse(fixture("outage")))
        assertEquals(PushMessage.Recovered, PushMessage.parse(fixture("recovered")))
    }

    private val train = PushMessage.Train(
        Line("L", Mode.TRAIN),
        mission = "PALO",
        destination = "Gare Centrale",
        from = "Les Aubépines",
        aimed = Instant.parse("2026-11-03T06:52:00Z"),
        expected = Instant.parse("2026-11-03T06:52:00Z"),
    )

    /** Ten minutes after the train was due. */
    private val trainGone = Instant.parse("2026-11-03T07:02:00Z")

    @Test
    fun readsACancelledTrainThatGoesAwayAfterItsTime() {
        assertEquals(
            PushMessage.TrainCancelled(train, PushMessage.Reason.MISSING, until = trainGone),
            PushMessage.parse(fixture("train_cancelled")),
        )
        val announced = PushMessage.parse(fixture("train_cancelled") + ("reason" to "announced"))
        assertEquals(PushMessage.Reason.ANNOUNCED, (announced as PushMessage.TrainCancelled).reason)
        // Whatever a newer backend's reason, the train is still cancelled.
        val other = PushMessage.parse(fixture("train_cancelled") + ("reason" to "something-new"))
        assertEquals(PushMessage.Reason.MISSING, (other as PushMessage.TrainCancelled).reason)
    }

    @Test
    fun readsATrainBack() {
        assertEquals(PushMessage.TrainBack(train, until = trainGone), PushMessage.parse(fixture("train_back")))
        val unnamed = PushMessage.parse(fixture("train_back") - "mission") as PushMessage.TrainBack
        assertNull(unnamed.train.mission)
    }

    @Test
    fun readsAReminderToLeave() {
        val late = train.copy(expected = Instant.parse("2026-11-03T06:54:00Z"))
        assertEquals(
            PushMessage.Leave(late, Instant.parse("2026-11-03T06:42:00Z"), "2", live = true, until = Instant.parse("2026-11-03T07:04:00Z")),
            PushMessage.parse(fixture("leave")),
        )
        val unknown = PushMessage.parse(fixture("leave") - "platform" - "live") as PushMessage.Leave
        assertNull(unknown.platform)
        assertEquals(false, unknown.live)
        assertNull(PushMessage.parse(fixture("leave") - "leave"))
        assertNull(PushMessage.parse(fixture("leave") - "line"))
        // An older backend sent no expected time: the timetable's stands for it.
        assertNull((PushMessage.parse(fixture("leave") - "expected") as PushMessage.Leave).train.expected)
    }

    @Test
    fun readsALateTrain() {
        assertEquals(
            PushMessage.TrainLate(train.copy(expected = Instant.parse("2026-11-03T06:56:00Z")), until = Instant.parse("2026-11-03T07:06:00Z")),
            PushMessage.parse(fixture("train_late")),
        )
        // Late by how much, it must say.
        assertNull(PushMessage.parse(fixture("train_late") - "expected"))
        assertNull(PushMessage.parse(fixture("train_late") - "from"))
    }

    @Test
    fun aTrainWithoutWhatNamesItIsIgnored() {
        for (missing in listOf("line", "destination", "from", "aimed")) {
            assertNull(missing, PushMessage.parse(fixture("train_cancelled") - missing))
            assertNull(missing, PushMessage.parse(fixture("train_back") - missing))
        }
        assertNull(PushMessage.parse(fixture("train_back") + ("aimed" to "soon")))
    }

    @Test
    fun aTrainBackReplacesItsCancellationOnly() {
        // The same tag as the backend's collapse key: neither a line's state nor another train.
        assertEquals("train-L-2026-11-03T06:52:00Z", PushMessage.parse(fixture("train_cancelled"))!!.tag)
        assertEquals("train-L-2026-11-03T06:52:00Z", PushMessage.parse(fixture("train_back"))!!.tag)
        assertEquals("train-L-2026-11-03T06:52:00Z", PushMessage.parse(fixture("leave"))!!.tag)
        assertEquals("train-L-2026-11-03T06:52:00Z", PushMessage.parse(fixture("train_late"))!!.tag)
    }

    @Test
    fun aLineOfAnotherModeIsJustALine() {
        assertEquals(
            PushMessage.Ended(Line("C09999", Mode.OTHER)),
            PushMessage.parse(mapOf("type" to "ended", "line" to "C09999", "mode" to "line")),
        )
    }

    @Test
    fun aStartWithoutWordsIsStillAStart() {
        assertEquals(
            PushMessage.Started(Line("L", Mode.TRAIN), Severity.DISRUPTED, title = null, text = null),
            PushMessage.parse(mapOf("type" to "started", "line" to "L", "mode" to "train", "severity" to "disrupted")),
        )
    }

    @Test
    fun anythingElseIsIgnored() {
        // A newer backend may send types or values this version does not know.
        val start = fixture("started")
        assertNull(PushMessage.parse(start + ("type" to "something-new")))
        assertNull(PushMessage.parse(start - "type"))
        assertNull(PushMessage.parse(start - "line"))
        assertNull(PushMessage.parse(start + ("severity" to "catastrophic")))
        assertNull(PushMessage.parse(fixture("changed") + ("previous" to "unknown")))
        assertNull(PushMessage.parse(fixture("changed") - "line"))
        assertNull(PushMessage.parse(fixture("ended") - "line"))
        assertNull(PushMessage.parse(fixture("planned") + ("count" to "two")))
        assertNull(PushMessage.parse(fixture("planned") - "titles"))
    }

    @Test
    fun eachLineHasItsOwnNotificationAndPlannedOnesShareOne() {
        // A change or an end replaces the line's previous notification instead of piling up.
        assertEquals("line-L", PushMessage.parse(fixture("started"))!!.tag)
        assertEquals("line-X", PushMessage.parse(fixture("changed"))!!.tag)
        assertEquals("line-L", PushMessage.parse(fixture("ended"))!!.tag)
        assertEquals("planned", PushMessage.parse(fixture("planned"))!!.tag)
        // Watching restored replaces watching interrupted.
        assertEquals("watch", PushMessage.Outage.tag)
        assertEquals("watch", PushMessage.Recovered.tag)
    }
}
