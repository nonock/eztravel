package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.notifications.PushMessage
import org.json.JSONObject
import java.time.Duration
import java.time.Instant

/**
 * The next departures on each leg of the way shown: the way to work until
 * its watch window ends, the way home after. The format is fixed by
 * `fixtures/api/departures.json` in the repository, which the backend tests read too.
 */
data class Departures(val direction: Status.Direction, val legs: List<Leg>) {
    /** The ride the reminder is for: the first with its time to the platform, as the backend has it. */
    val reminded: Leg?
        get() = legs.firstOrNull { it.reach != null }

    /** The train shown first, on [leg], the ride reminded, and when to leave for it. */
    data class Featured(val leg: Leg, val departure: Departure, val leave: Instant)

    /**
     * The train shown first on the ride reminded: the one picked, unless
     * cancelled, or else the next one that can be caught at [now].
     */
    fun featured(now: Instant): Featured? {
        for (leg in legs) {
            val reach = leg.reach ?: continue
            val departures = leg.departures.orEmpty()
            val departure = departures.firstOrNull { it.picked && it.state != State.CANCELLED }
                ?: departures.firstOrNull { leg.catchable(it, now) }
                ?: return null
            return Featured(leg, departure, departure.expected.minus(reach))
        }
        return null
    }

    /** The same departures, with [journey] as the one picked, or none. */
    fun picking(journey: String?) = copy(
        legs = legs.map { leg ->
            leg.copy(departures = leg.departures?.map { it.copy(picked = journey != null && it.journey == journey) })
        },
    )

    /**
     * `from` and `to` are null until the backend knows the line's stops;
     * `departures` is null when they cannot be read. `reach` is the time from
     * leaving home, or work, to the platform, when configured.
     */
    data class Leg(
        val line: PushMessage.Line,
        val color: Int?,
        val from: String?,
        val to: String?,
        val departures: List<Departure>?,
        val reach: Duration? = null,
    ) {
        /** When to leave for [departure], from its expected time; null without a time to the platform. */
        fun leaveAt(departure: Departure): Instant? = reach?.let { departure.expected.minus(it) }

        /** Not cancelled, and its time to leave not past at [now]. */
        fun catchable(departure: Departure, now: Instant): Boolean =
            departure.state != State.CANCELLED && leaveAt(departure)?.let { !now.isAfter(it) } == true
    }

    data class Departure(
        /** Null on the metro, which gives expected times only. */
        val aimed: Instant?,
        val expected: Instant,
        /** At the alight stop, when known. */
        val arrival: Instant?,
        val mission: String?,
        val destination: String,
        val platform: String?,
        val state: State,
        /** The train or metro, the same at every stop of its run; null from an older backend. */
        val journey: String? = null,
        /** Picked for the reminder to leave. */
        val picked: Boolean = false,
    ) {
        /** The time it is known by: the timetable's, or the expected one on the metro. */
        val knownBy: Instant
            get() = aimed ?: expected

        /** Whole minutes late; 0 when on time or early. */
        val delayMinutes: Long
            get() = aimed?.let { Duration.between(it, expected).toMinutes().coerceAtLeast(0) } ?: 0
    }

    /** As the operator says: a train can be said late before its time moves. */
    enum class State { ON_TIME, DELAYED, CANCELLED }

    companion object {
        fun parse(json: String): Departures {
            val departures = JSONObject(json)
            return Departures(
                if (departures.optString("direction") == "inbound") Status.Direction.INBOUND else Status.Direction.OUTBOUND,
                departures.getJSONArray("legs").objects().map { leg ->
                    Leg(
                        PushMessage.Line(leg.getString("line"), PushMessage.mode(leg.getString("mode"))),
                        leg.stringOrNull("color")?.let(::hexColor),
                        leg.stringOrNull("from"),
                        leg.stringOrNull("to"),
                        if (leg.isNull("departures")) null else leg.getJSONArray("departures").objects().map(::departure),
                        if (leg.isNull("reach")) null else Duration.ofMinutes(leg.getLong("reach")),
                    )
                },
            )
        }

        private fun departure(json: JSONObject) = Departure(
            aimed = json.instantOrNull("aimed"),
            expected = Instant.parse(json.getString("expected")),
            arrival = json.instantOrNull("arrival"),
            mission = json.stringOrNull("mission"),
            destination = json.getString("destination"),
            platform = json.stringOrNull("platform"),
            state = when (json.getString("status")) {
                "cancelled" -> State.CANCELLED
                "delayed" -> State.DELAYED
                else -> State.ON_TIME
            },
            journey = json.stringOrNull("journey"),
            picked = json.optBoolean("picked"),
        )
    }
}

/**
 * Why the backend will not remind of a picked train. The format is fixed by
 * `fixtures/api/pick-refused.json` in the repository.
 */
enum class Refusal {
    /** Its time to leave is past. */
    TOO_LATE,

    /** Not an office day, or time away. */
    NOT_OFFICE_DAY,

    /** Leaving outside the watch window, when no reminder is sent. */
    OUTSIDE_WINDOW,

    /** No longer shown, or cancelled. */
    UNKNOWN,

    /** The backend cannot read the departures just now. */
    UNAVAILABLE,
    ;

    companion object {
        /** Null when the answer gives no reason this version knows. */
        fun parse(json: String): Refusal? {
            val name = runCatching { JSONObject(json) }.getOrNull()?.stringOrNull("refused")
            return entries.firstOrNull { it.name.lowercase() == name }
        }
    }
}
