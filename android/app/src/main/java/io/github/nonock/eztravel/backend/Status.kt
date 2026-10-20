package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.notifications.PushMessage
import org.json.JSONObject
import java.time.Duration
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

    /** `severity` is null when the line runs normally; `color` (ARGB) when the backend has one. */
    data class LineState(val line: PushMessage.Line, val severity: PushMessage.Severity?, val color: Int? = null)

    /** What the home screen says first. */
    sealed interface Headline {
        /** Checks fail: nothing can be said of the lines. */
        data class Out(val outage: Outage) : Headline

        /** The worst line in trouble. */
        data class Trouble(val line: LineState) : Headline

        /** Watched, just checked, every line running. */
        data object Fine : Headline

        /** Watched, but no recent check to vouch for anything yet. */
        data object Checking : Headline

        /** Outside the watch windows. */
        data object Idle : Headline
    }

    /** Fine only when it is known: during a window, checked within the last few minutes, no line in trouble. */
    fun headline(now: Instant): Headline = when {
        outage != null -> Headline.Out(outage)
        watching == null -> Headline.Idle
        else -> lines.firstOrNull { it.severity == PushMessage.Severity.BLOCKING }?.let { Headline.Trouble(it) }
            ?: lines.firstOrNull { it.severity != null }?.let { Headline.Trouble(it) }
            ?: if (checkedAt != null && checkedAt.isAfter(now.minus(FRESH))) Headline.Fine else Headline.Checking
    }

    companion object {
        /** Checks run every two minutes: a check older than this is not a fresh one. */
        private val FRESH: Duration = Duration.ofMinutes(5)

        /** Values a newer backend may add fall back on the safe side: trouble stays trouble. */
        fun parse(json: String): Status {
            val status = JSONObject(json)
            return Status(
                watching = when (status.optString("watching")) {
                    "outbound" -> Direction.OUTBOUND
                    "inbound" -> Direction.INBOUND
                    else -> null
                },
                checkedAt = status.instantOrNull("checked_at"),
                next = status.instantOrNull("next"),
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
                        line.stringOrNull("color")?.let(::hexColor),
                    )
                },
                planned = status.getJSONArray("planned").let { titles -> List(titles.length()) { titles.getString(it) } },
            )
        }
    }
}
