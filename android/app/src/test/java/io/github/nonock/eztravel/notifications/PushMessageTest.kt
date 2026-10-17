package io.github.nonock.eztravel.notifications

import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.io.File

class PushMessageTest {
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
            ),
            PushMessage.parse(fixture("changed")),
        )
    }

    @Test
    fun readsAnEnd() {
        assertEquals(PushMessage.Ended(Line("L", Mode.TRAIN)), PushMessage.parse(fixture("ended")))
    }

    @Test
    fun readsPlannedDisruptions() {
        assertEquals(
            PushMessage.Planned(2, "Ligne L : offre adaptée\nMétro X : station fermée"),
            PushMessage.parse(fixture("planned")),
        )
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
    }
}
