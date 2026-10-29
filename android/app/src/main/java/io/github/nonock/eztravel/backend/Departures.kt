package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.notifications.PushMessage
import org.json.JSONObject
import java.time.Duration
import java.time.Instant
import kotlin.math.cos
import kotlin.math.sqrt

/**
 * The next departures on each leg of the way shown: the way to work until
 * its watch window ends, the way home after; and its [backups] needed now.
 * The format is fixed by `fixtures/api/departures.json` in the repository,
 * which the backend tests read too.
 */
data class Departures(val direction: Status.Direction, val legs: List<Leg>, val backups: List<Backup> = emptyList()) {
    /** The ride the reminder is for: the first with its time to the platform, as the backend has it. */
    val reminded: Leg?
        get() = legs.firstOrNull { it.reach != null }

    /**
     * Rides on other lines that stand in for [insteadOf], lines of the way
     * disrupted now; a ride can be disrupted too. Never picked.
     */
    data class Backup(val insteadOf: List<Status.LineState>, val legs: List<Leg>)

    /**
     * The rides from the one boarding nearest [here] on, then those behind:
     * at the station of the second ride, it comes first. In the way's order
     * without a position, or while a ride's stop is not placed.
     */
    fun nearestFirst(here: Position?): List<Leg> {
        if (here == null || legs.any { it.fromPosition == null }) return legs
        val nearest = legs.indices.minByOrNull { here.distanceTo(legs[it].fromPosition!!) } ?: return legs
        return legs.drop(nearest) + legs.take(nearest)
    }

    /** The train shown first, on [leg], the ride reminded, and when to leave for it. */
    data class Featured(val leg: Leg, val departure: Departure, val leave: Instant)

    /**
     * The train shown first on the ride reminded: the one picked, unless
     * cancelled, or else the usual one, or else the next one that can be
     * caught at [now].
     */
    fun featured(now: Instant): Featured? {
        for (leg in legs) {
            val reach = leg.reach ?: continue
            val departures = leg.departures.orEmpty()
            val departure = departures.firstOrNull { it.picked && it.state != State.CANCELLED }
                ?: departures.firstOrNull { it.usual && it.state != State.CANCELLED }
                ?: departures.firstOrNull { leg.catchable(it, now) }
                ?: return null
            return Featured(leg, departure, departure.expected.minus(reach))
        }
        return null
    }

    /** The same departures, with [journey] as the one picked, or none: a pick replaces the usual train. */
    fun picking(journey: String?) = copy(
        legs = legs.map { leg ->
            leg.copy(
                departures = leg.departures?.map {
                    it.copy(picked = journey != null && it.journey == journey, usual = it.usual && journey == null)
                },
            )
        },
    )

    /**
     * `from` and `to` are null until the backend knows the line's stops;
     * `departures` is null when they cannot be read. `reach` is the time from
     * leaving home, or work, to the platform, when configured. A backup's ride
     * has its line's [severity] when disrupted too.
     */
    data class Leg(
        val line: PushMessage.Line,
        val color: Int?,
        val from: String?,
        val to: String?,
        val departures: List<Departure>?,
        val reach: Duration? = null,
        val severity: PushMessage.Severity? = null,
        /** Where [from] is, once the backend knows. */
        val fromPosition: Position? = null,
        /** Its departures are its timetable's: its stops cannot be read live. */
        val timetable: Boolean = false,
    ) {
        /** When to leave for [departure], from its expected time; null without a time to the platform. */
        fun leaveAt(departure: Departure): Instant? = reach?.let { departure.expected.minus(it) }

        /** Not cancelled, and its time to leave not past at [now]. */
        fun catchable(departure: Departure, now: Instant): Boolean =
            departure.state != State.CANCELLED && leaveAt(departure)?.let { !now.isAfter(it) } == true

        /**
         * The departures shown before "more": with a time to the platform, in
         * time order up to the [CATCHABLE]th that can be caught at [now], so
         * that those too soon come first for whoever is already on the way;
         * without, the first [FIRST]. The picked and usual trains always.
         */
        fun shortList(now: Instant): List<Departure> {
            val all = departures.orEmpty()
            val shown = if (reach == null) {
                FIRST
            } else {
                var caught = 0
                all.indexOfFirst { catchable(it, now) && ++caught == CATCHABLE } + 1
            }
            if (shown == 0) return all
            return all.filterIndexed { index, departure -> index < shown || departure.picked || departure.usual }
        }

        private companion object {
            const val CATCHABLE = 3
            const val FIRST = 4
        }
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
        /** The way's usual train, when none is picked that day. */
        val usual: Boolean = false,
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
                departures.getJSONArray("legs").objects().map(::leg),
                // None from a backend without backups.
                departures.optJSONArray("backups")?.objects().orEmpty().map { backup ->
                    Backup(
                        backup.getJSONArray("instead_of").objects().map(Status::lineState),
                        backup.getJSONArray("legs").objects().map(::leg),
                    )
                },
            )
        }

        private fun leg(json: JSONObject) = Leg(
            PushMessage.Line(json.getString("line"), PushMessage.mode(json.getString("mode"))),
            json.stringOrNull("color")?.let(::hexColor),
            json.stringOrNull("from"),
            json.stringOrNull("to"),
            if (json.isNull("departures")) null else json.getJSONArray("departures").objects().map(::departure),
            if (json.isNull("reach")) null else Duration.ofMinutes(json.getLong("reach")),
            Status.severity(json),
            json.optJSONObject("from_position")?.let { Position(it.getDouble("lat"), it.getDouble("lon")) },
            json.optBoolean("timetable"),
        )

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
            usual = json.optBoolean("usual"),
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

    /** Not an office day, or a day off. */
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

/** A place in decimal degrees: a stop, or the phone, which keeps its own. */
data class Position(val lat: Double, val lon: Double) {
    /** Near enough to compare stops a few kilometres apart: degrees of longitude shrink away from the equator. */
    fun distanceTo(other: Position): Double {
        val east = (other.lon - lon) * cos(Math.toRadians((lat + other.lat) / 2))
        val north = other.lat - lat
        return sqrt(east * east + north * north)
    }
}
