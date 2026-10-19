package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.notifications.PushMessage
import org.json.JSONObject
import java.time.Instant

/**
 * What the backend sent, or a watch window that closed on a disruption.
 * The format is fixed by `fixtures/api/history.json` in the repository.
 */
sealed interface HistoryEntry {
    val at: Instant

    /** A notification, as the phone received it. */
    data class Sent(override val at: Instant, val message: PushMessage) : HistoryEntry

    /** The window closed while [line] was still disrupted: no "back to normal" followed. */
    data class WindowClosed(override val at: Instant, val line: PushMessage.Line, val severity: PushMessage.Severity) : HistoryEntry

    companion object {
        /** Newest first, as sent; entries this version cannot read are left out. */
        fun parse(json: String): List<HistoryEntry> {
            val entries = JSONObject(json).getJSONArray("entries")
            return List(entries.length()) { entries.getJSONObject(it) }.mapNotNull { entry ->
                val at = Instant.parse(entry.getString("at"))
                val data = entry.getJSONObject("data").let { data -> data.keys().asSequence().associateWith { data.getString(it) } }
                if (data["type"] == "window_closed") {
                    val line = data["line"] ?: return@mapNotNull null
                    WindowClosed(
                        at,
                        PushMessage.Line(line, PushMessage.mode(data["mode"])),
                        if (data["severity"] == "blocking") PushMessage.Severity.BLOCKING else PushMessage.Severity.DISRUPTED,
                    )
                } else {
                    PushMessage.parse(data)?.let { Sent(at, it) }
                }
            }
        }
    }
}
