package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.notifications.PushMessage
import org.json.JSONArray
import org.json.JSONObject
import java.time.Instant

/**
 * What the backend sees, for the home screen. Its format is fixed by
 * `fixtures/api/` in the repository, which the backend tests read too.
 */
data class Status(
    /** Null outside the watch windows. */
    val watching: Direction?,
    /** The last complete check: PRIM read and every notification sent. */
    val checkedAt: Instant?,
    /** When the next watch window opens. */
    val next: Instant?,
    val outage: Outage?,
    /** The lines of the way being watched. */
    val lines: List<LineState>,
    /** The planned disruptions announced today. */
    val planned: List<String>,
) {
    enum class Direction { OUTBOUND, INBOUND }

    enum class Cause { PRIM, FIREBASE, SERVER }

    /** No check has been complete since `since`. */
    data class Outage(val since: Instant, val cause: Cause)

    /** `severity` is null when the line runs normally. */
    data class LineState(val line: PushMessage.Line, val severity: PushMessage.Severity?)

    companion object {
        /** Values a newer backend may add fall back on the safe side: trouble stays trouble. */
        fun parse(json: String): Status {
            val status = JSONObject(json)
            return Status(
                watching = when (status.optString("watching")) {
                    "outbound" -> Direction.OUTBOUND
                    "inbound" -> Direction.INBOUND
                    else -> null
                },
                checkedAt = status.instant("checked_at"),
                next = status.instant("next"),
                outage = if (status.isNull("outage")) {
                    null
                } else {
                    val outage = status.getJSONObject("outage")
                    Outage(
                        Instant.parse(outage.getString("since")),
                        when (outage.getString("cause")) {
                            "prim" -> Cause.PRIM
                            "firebase" -> Cause.FIREBASE
                            else -> Cause.SERVER
                        },
                    )
                },
                lines = status.getJSONArray("lines").objects().map { line ->
                    LineState(
                        PushMessage.Line(line.getString("line"), PushMessage.mode(line.getString("mode"))),
                        when {
                            line.isNull("severity") -> null
                            line.getString("severity") == "blocking" -> PushMessage.Severity.BLOCKING
                            else -> PushMessage.Severity.DISRUPTED
                        },
                    )
                },
                planned = status.getJSONArray("planned").let { titles -> List(titles.length()) { titles.getString(it) } },
            )
        }

        // Android's org.json may read a null as the text "null": check for it first.
        private fun JSONObject.instant(name: String): Instant? = if (isNull(name)) null else Instant.parse(getString(name))

        private fun JSONArray.objects(): List<JSONObject> = List(length()) { getJSONObject(it) }
    }
}
