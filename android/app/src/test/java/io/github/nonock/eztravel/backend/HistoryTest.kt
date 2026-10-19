package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.backend.HistoryEntry.Sent
import io.github.nonock.eztravel.backend.HistoryEntry.WindowClosed
import io.github.nonock.eztravel.notifications.PushMessage
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test
import java.io.File
import java.time.Instant

class HistoryTest {
    /** The answer the backend gives, shared with its own tests. */
    private val fixture = File("../../fixtures/api/history.json").readText()

    private fun at(instant: String) = Instant.parse(instant)

    @Test
    fun readsTheHistoryNewestFirst() {
        assertEquals(
            listOf(
                WindowClosed(at("2026-11-03T07:15:00Z"), Line("X", Mode.METRO), Severity.DISRUPTED),
                Sent(
                    at("2026-11-03T06:40:00Z"),
                    PushMessage.Started(
                        Line("X", Mode.METRO),
                        Severity.DISRUPTED,
                        "Métro X : trafic perturbé",
                        "Le trafic est perturbé sur l'ensemble de la ligne.",
                    ),
                ),
                Sent(at("2026-11-03T06:20:00Z"), PushMessage.Ended(Line("L", Mode.TRAIN))),
                Sent(
                    at("2026-11-03T06:00:00Z"),
                    PushMessage.Started(
                        Line("L", Mode.TRAIN),
                        Severity.BLOCKING,
                        "Ligne L : Nanterre <-> Cergy trafic interrompu",
                        "Le trafic est interrompu entre Nanterre et Cergy jusqu'à 20h.\nMotif : panne d'un train.",
                    ),
                ),
                Sent(at("2026-11-03T05:46:00Z"), PushMessage.Planned(1, "Ligne L : offre adaptée")),
            ),
            HistoryEntry.parse(fixture),
        )
    }

    private fun withEntries(vararg data: JSONObject): String {
        val entries = JSONArray()
        for (each in data) entries.put(JSONObject().put("at", "2026-11-03T07:15:00Z").put("data", each))
        return JSONObject().put("entries", entries).toString()
    }

    @Test
    fun aWindowClosingOnAnInterruptionSaysSo() {
        val closed = JSONObject().put("type", "window_closed").put("line", "L").put("mode", "train").put("severity", "blocking")
        assertEquals(
            listOf(WindowClosed(at("2026-11-03T07:15:00Z"), Line("L", Mode.TRAIN), Severity.BLOCKING)),
            HistoryEntry.parse(withEntries(closed)),
        )
    }

    @Test
    fun entriesThisVersionCannotReadAreLeftOut() {
        // A newer backend may record kinds this version does not know.
        val unknown = JSONObject().put("type", "something-new")
        val lineless = JSONObject().put("type", "window_closed").put("mode", "train").put("severity", "blocking")
        assertEquals(emptyList<HistoryEntry>(), HistoryEntry.parse(withEntries(unknown, lineless)))
    }
}
