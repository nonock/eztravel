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
    )

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
    ) {
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
        )
    }
}
