package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.notifications.PushMessage
import org.json.JSONObject
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime

/** How the trains kept ran, since the first day kept: `/stats`. */
data class Stats(val since: LocalDate?, val ways: List<Way>) {
    /** Of [trains], those on time (less than five minutes behind), late and cancelled. */
    data class Count(val trains: Int, val onTime: Int, val late: Int, val cancelled: Int) {
        /** The share on time, in whole percents rounded down; none with no train. */
        val onTimePercent: Int
            get() = if (trains == 0) 0 else onTime * 100 / trains
    }

    /** A way's [line], colored as on the home screen, and its [usual] train by its time, when set; the latest day first. */
    data class Way(
        val direction: Status.Direction,
        val line: PushMessage.Line,
        val color: Int?,
        val usual: LocalTime?,
        val count: Count,
        val days: List<Day>,
    )

    /** One office day, with the usual train of that day, if one ran then. */
    data class Day(val day: LocalDate, val count: Count, val usual: UsualRun?)

    /** The usual train of a day: leaving [aimed], it ran [run], [late] whole minutes. */
    data class UsualRun(val aimed: Instant, val run: Run, val late: Int)

    enum class Run { ON_TIME, LATE, CANCELLED }

    companion object {
        fun parse(json: String): Stats {
            val stats = JSONObject(json)
            return Stats(
                stats.stringOrNull("since")?.let(LocalDate::parse),
                stats.getJSONArray("ways").objects().map { way ->
                    Way(
                        if (way.getString("direction") == "inbound") Status.Direction.INBOUND else Status.Direction.OUTBOUND,
                        PushMessage.Line(way.getString("line"), PushMessage.mode(way.getString("mode"))),
                        way.stringOrNull("color")?.let(::hexColor),
                        way.stringOrNull("usual")?.let(LocalTime::parse),
                        count(way),
                        way.getJSONArray("days").objects().map { day ->
                            Day(
                                LocalDate.parse(day.getString("day")),
                                count(day),
                                day.optJSONObject("usual")?.let { usual ->
                                    UsualRun(Instant.parse(usual.getString("aimed")), run(usual.getString("run")), usual.getInt("late"))
                                },
                            )
                        },
                    )
                },
            )
        }

        private fun count(json: JSONObject) =
            Count(json.getInt("trains"), json.getInt("on_time"), json.getInt("late"), json.getInt("cancelled"))

        /** A run a newer backend names otherwise is taken as late: trouble stays trouble. */
        private fun run(name: String) = when (name) {
            "on_time" -> Run.ON_TIME
            "cancelled" -> Run.CANCELLED
            else -> Run.LATE
        }
    }
}
