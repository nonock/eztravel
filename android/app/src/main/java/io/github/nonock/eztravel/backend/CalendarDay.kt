package io.github.nonock.eztravel.backend

import org.json.JSONObject
import java.time.LocalDate

/**
 * A day of the calendar: what it is, and why, and the school holidays it is
 * in, if any. Only office days are watched.
 */
data class CalendarDay(val date: LocalDate, val kind: Kind, val why: Why, val schoolHoliday: String? = null) {
    enum class Kind {
        OFFICE,

        /** A weekday working from home. */
        REMOTE,

        /** A day off. */
        AWAY,

        /** A weekend day or a public holiday: cannot be set. */
        OFF,
    }

    enum class Why {
        /** The two weeks' office days. */
        CYCLE,
        HOLIDAY,

        /** Set from the app. */
        SET,
    }

    companion object {
        /**
         * From `GET /calendar`, in the shape fixed by `fixtures/api/calendar.json`.
         * A kind this version does not know is not watched; a reason it does not know is the plan.
         */
        fun parse(json: String): List<CalendarDay> = JSONObject(json).getJSONArray("days").objects().map { day ->
            CalendarDay(
                LocalDate.parse(day.getString("date")),
                Kind.entries.firstOrNull { it.name.lowercase() == day.optString("kind") } ?: Kind.OFF,
                Why.entries.firstOrNull { it.name.lowercase() == day.optString("why") } ?: Why.CYCLE,
                day.stringOrNull("school_holiday"),
            )
        }
    }
}

/** Why the backend will not set a day, in the shape fixed by `fixtures/api/day-refused.json`. */
enum class DayRefusal {
    /** Before today. */
    PAST,

    /** A year ahead or more, which the backend never looks at. */
    TOO_FAR,
    ;

    companion object {
        /** Null when the answer gives no reason this version knows. */
        fun parse(json: String): DayRefusal? {
            val name = runCatching { JSONObject(json) }.getOrNull()?.stringOrNull("refused")
            return entries.firstOrNull { it.name.lowercase() == name }
        }
    }
}
