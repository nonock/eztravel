package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.backend.Alerts.Alert
import io.github.nonock.eztravel.backend.Alerts.Cause
import io.github.nonock.eztravel.backend.Alerts.Link
import io.github.nonock.eztravel.backend.Alerts.Severity
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.io.File
import java.time.Instant

class AlertsTest {
    /** The answers the backend gives, shared with its own tests. */
    private fun fixture(name: String) = File("../../fixtures/api/$name.json").readText()

    private fun at(instant: String) = Instant.parse(instant)

    @Test
    fun readsTheAlertsOfEachLine() {
        val (train, metro) = Alerts.parse(fixture("alerts")).lines!!
        assertEquals(Line("L", Mode.TRAIN), train.line)
        assertEquals(0xFF7B61C9.toInt(), train.color)
        assertEquals(
            Alert(
                id = "incident-l",
                severity = Severity.BLOCKING,
                cause = Cause.INCIDENT,
                title = "Ligne L : trafic interrompu entre Les Aubépines et Gare Centrale",
                text = "Le trafic est interrompu entre Les Aubépines et Gare Centrale jusqu'à 9h.\nMotif : panne d'un train.",
                active = true,
                begin = at("2026-11-03T06:30:00Z"),
                end = at("2026-11-03T08:00:00Z"),
                onWay = true,
                updated = at("2026-11-03T06:35:00Z"),
            ),
            train.alerts[0],
        )
        assertEquals(listOf(Severity.BLOCKING, Severity.INFORMATION), train.alerts.map { it.severity })
        assertEquals(listOf(Cause.INCIDENT, Cause.WORKS), train.alerts.map { it.cause })
        assertEquals(listOf(true, false), train.alerts.map { it.active })
        assertEquals(listOf(true, false), train.alerts.map { it.onWay })
        assertEquals(listOf(Link(96, 112, "https://example.org/travaux-l")), train.alerts[1].links)
        assertEquals("site de la ligne", train.alerts[1].text.substring(96, 112))
        assertEquals(Line("X", Mode.METRO), metro.line)
        assertEquals(Severity.DISRUPTED to Cause.UNKNOWN, metro.alerts[0].severity to metro.alerts[0].cause)
    }

    @Test
    fun linksAreReadAsIndicesOfTheTextAndOnlyThoseSafeToOpen() {
        val json = JSONObject(fixture("alerts"))
        val first = json.getJSONArray("lines").getJSONObject(0).getJSONArray("alerts").getJSONObject(0)
        // The backend counts code points: the train is one, but two chars in Kotlin.
        first.put("text", "\uD83D\uDE86 Détails ici, là, ou ailleurs.")
        val link = { start: Int, end: Int, url: String -> JSONObject().put("start", start).put("end", end).put("url", url) }
        first.put(
            "links",
            org.json.JSONArray()
                .put(link(10, 13, "https://example.org/ici"))
                .put(link(15, 17, "http://example.org/la"))
                .put(link(22, 99, "https://example.org/ailleurs"))
                .put(link(13, 13, "https://example.org/vide"))
                .put(link(-1, 2, "https://example.org/avant")),
        )
        val alert = Alerts.parse(json.toString()).lines!![0].alerts[0]
        assertEquals(listOf(Link(11, 14, "https://example.org/ici")), alert.links)
        assertEquals("ici", alert.text.substring(11, 14))
        // From a backend without links.
        first.remove("links")
        assertEquals(emptyList<Link>(), Alerts.parse(json.toString()).lines!![0].alerts[0].links)
    }

    @Test
    fun alertsThatCannotBeReadAreUnknownNotNone() {
        assertNull(Alerts.parse("""{"lines":null}""").lines)
    }

    @Test
    fun valuesFromANewerBackendFallBackSafely() {
        val json = JSONObject(fixture("alerts"))
        val first = json.getJSONArray("lines").getJSONObject(0).getJSONArray("alerts").getJSONObject(0)
        first.put("severity", "catastrophic").put("cause", "strike").put("information", "x")
        first.remove("on_way")
        val alert = Alerts.parse(json.toString()).lines!![0].alerts[0]
        assertEquals(Severity.DISRUPTED to Cause.UNKNOWN, alert.severity to alert.cause)
        assertEquals(false, alert.onWay)
        json.getJSONArray("lines").getJSONObject(1).put("color", JSONObject.NULL)
        assertNull(Alerts.parse(json.toString()).lines!![1].color)
        // The cause "information" too.
        first.put("cause", "information")
        assertEquals(Cause.INFORMATION, Alerts.parse(json.toString()).lines!![0].alerts[0].cause)
    }

    @Test
    fun aLineSaysHowManyAreUnderWayAndToComeAndTheWorstUnderWay() {
        val (train, metro) = Alerts.parse(fixture("alerts")).lines!!
        val works = train.alerts[1]
        val split = train.copy(alerts = train.alerts + works.copy(id = "works-l-2"))
        // Counted as shown: alerts sharing a title once.
        assertEquals(1 to 1, split.underWay to split.toCome)
        assertEquals(Severity.BLOCKING, split.worstUnderWay)
        assertEquals(Severity.DISRUPTED, metro.worstUnderWay)
        // Works to come only: nothing under way.
        val coming = train.copy(alerts = listOf(works))
        assertEquals(0 to 1, coming.underWay to coming.toCome)
        assertNull(coming.worstUnderWay)
    }

    @Test
    fun alertsSharingATitleShowOnce() {
        val line = Alerts.parse(fixture("alerts")).lines!![0]
        val works = line.alerts[1]
        val split = line.copy(alerts = line.alerts + works.copy(id = "works-l-2") + works.copy(id = "works-l-3"))
        assertEquals(listOf(line.alerts[0] to 0, works to 2), split.grouped)
    }

    @Test
    fun aNotificationOpensTheTitleOfItsRecord() {
        val line = Alerts.parse(fixture("alerts")).lines!![0]
        val (incident, works) = line.alerts
        val split = line.copy(alerts = line.alerts + works.copy(id = "works-l-2"))
        // By its id, though it shows under the first of its title.
        assertEquals(works.title, split.unfolded("works-l-2", "An older title"))
        // Edited since, under another id: by its title.
        assertEquals(incident.title, split.unfolded("gone", incident.title))
        // Neither: nothing unfolded.
        assertNull(split.unfolded("gone", "An older title"))
        assertNull(split.unfolded(null, null))
    }
}
