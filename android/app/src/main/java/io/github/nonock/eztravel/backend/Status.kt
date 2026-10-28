package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.notifications.PushMessage
import org.json.JSONObject
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime

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
    /** The name of the school holidays today is in, if any: fewer trains run. */
    val schoolHoliday: String? = null,
    /** Their first and last days, from a backend that gives them. */
    val schoolHolidayFirst: LocalDate? = null,
    val schoolHolidayLast: LocalDate? = null,
    /** Today's way home, when later than usual. */
    val late: Late? = null,
) {
    enum class Direction { OUTBOUND, INBOUND }

    enum class Cause { PRIM, FIREBASE, SERVER }

    /** No check has been complete since `since`. */
    data class Outage(val since: Instant, val cause: Cause)

    /**
     * A way home later than usual: when it leaves and until when it is
     * watched, local; [set] from the app, or else by the backend's rules.
     */
    data class Late(val leave: LocalTime, val until: LocalTime, val set: Boolean)

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
                lines = status.getJSONArray("lines").objects().map(::lineState),
                planned = status.getJSONArray("planned").let { titles -> List(titles.length()) { titles.getString(it) } },
                schoolHoliday = status.stringOrNull("school_holiday"),
                schoolHolidayFirst = status.stringOrNull("school_holiday_first")?.let(LocalDate::parse),
                schoolHolidayLast = status.stringOrNull("school_holiday_last")?.let(LocalDate::parse),
                late = status.optJSONObject("late")?.let {
                    Late(LocalTime.parse(it.getString("leave")), LocalTime.parse(it.getString("until")), it.getBoolean("set"))
                },
            )
        }

        /** A line as `/status` gives it, and the backups of `/departures`. */
        internal fun lineState(line: JSONObject) = LineState(
            PushMessage.Line(line.getString("line"), PushMessage.mode(line.getString("mode"))),
            severity(line),
            line.stringOrNull("color")?.let(::hexColor),
        )

        /** A `severity` the backend gives, none when absent or null: an unknown one is a disruption. */
        internal fun severity(json: JSONObject): PushMessage.Severity? = when {
            json.isNull("severity") -> null
            json.getString("severity") == "blocking" -> PushMessage.Severity.BLOCKING
            else -> PushMessage.Severity.DISRUPTED
        }
    }
}
