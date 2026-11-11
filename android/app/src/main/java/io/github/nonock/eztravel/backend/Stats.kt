package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.notifications.PushMessage
import org.json.JSONObject
import java.time.DayOfWeek
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime

/**
 * How the trains kept ran, since the first day kept, and the lines told
 * disrupted: `/stats`. A backend from before [Way.times], [Way.weekdays] and
 * [disruptions] leaves them empty, or none.
 */
data class Stats(val since: LocalDate?, val ways: List<Way>, val disruptions: Disruptions? = null) {
    /** Of [trains], those on time (less than five minutes behind), late and cancelled. */
    data class Count(val trains: Int, val onTime: Int, val late: Int, val cancelled: Int) {
        /** The share on time, in whole percents rounded down; none with no train. */
        val onTimePercent: Int
            get() = if (trains == 0) 0 else onTime * 100 / trains
    }

    /**
     * A way's [line], colored as on the home screen, and its [usual] train by its time, when set; the latest day
     * first. Its [times], each departure kept one day in two at least, the earliest first; its [weekdays], Monday first.
     */
    data class Way(
        val direction: Status.Direction,
        val line: PushMessage.Line,
        val color: Int?,
        val usual: LocalTime?,
        val count: Count,
        val days: List<Day>,
        val times: List<Slot> = emptyList(),
        val weekdays: List<Weekday> = emptyList(),
    )

    /** One office day, with the usual train of that day, if one ran then. */
    data class Day(val day: LocalDate, val count: Count, val usual: UsualRun?)

    /** The trains leaving at [time]. */
    data class Slot(val time: LocalTime, val count: Count)

    /** The trains of a weekday, over the [days] of it with a train kept. */
    data class Weekday(val day: DayOfWeek, val days: Int, val count: Count)

    /** The usual train of a day: leaving [aimed], it ran [run], [late] whole minutes. */
    data class UsualRun(val aimed: Instant, val run: Run, val late: Int)

    enum class Run { ON_TIME, LATE, CANCELLED }

    /** The lines told disrupted, from the oldest day the history keeps, [since]. */
    data class Disruptions(val since: LocalDate?, val lines: List<Disrupted>)

    /** A [line] with an alert on [days] days, [alerts] in all: a disruption begun, or trains late with no record. */
    data class Disrupted(val line: PushMessage.Line, val color: Int?, val days: Int, val alerts: Int)

    companion object {
        fun parse(json: String): Stats {
            val stats = JSONObject(json)
            return Stats(
                stats.stringOrNull("since")?.let(LocalDate::parse),
                stats.getJSONArray("ways").objects().map { way ->
                    Way(
                        if (way.getString("direction") == "inbound") Status.Direction.INBOUND else Status.Direction.OUTBOUND,
                        line(way),
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
                        way.optJSONArray("times")?.objects().orEmpty().map { Slot(LocalTime.parse(it.getString("time")), count(it)) },
                        way.optJSONArray("weekdays")?.objects().orEmpty().map {
                            Weekday(DayOfWeek.of(it.getInt("weekday")), it.getInt("days"), count(it))
                        },
                    )
                },
                stats.optJSONObject("disruptions")?.let { disruptions ->
                    Disruptions(
                        disruptions.stringOrNull("since")?.let(LocalDate::parse),
                        disruptions.getJSONArray("lines").objects().map {
                            Disrupted(line(it), it.stringOrNull("color")?.let(::hexColor), it.getInt("days"), it.getInt("alerts"))
                        },
                    )
                },
            )
        }

        private fun line(json: JSONObject) = PushMessage.Line(json.getString("line"), PushMessage.mode(json.getString("mode")))

        internal fun count(json: JSONObject) =
            Count(json.getInt("trains"), json.getInt("on_time"), json.getInt("late"), json.getInt("cancelled"))

        /** A run a newer backend names otherwise is taken as late: trouble stays trouble. */
        private fun run(name: String) = when (name) {
            "on_time" -> Run.ON_TIME
            "cancelled" -> Run.CANCELLED
            else -> Run.LATE
        }
    }
}
