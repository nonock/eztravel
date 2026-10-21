package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.backend.Status.Direction
import org.json.JSONObject
import java.time.LocalTime

/**
 * The usual train of each way, by its time at the station of the ride the
 * reminder is for: on office days, the reminder to leave is for the first
 * train from then, unless one is picked that day.
 */
data class UsualTrains(val outbound: LocalTime?, val inbound: LocalTime?) {
    fun of(direction: Direction): LocalTime? = when (direction) {
        Direction.OUTBOUND -> outbound
        Direction.INBOUND -> inbound
    }

    fun with(direction: Direction, time: LocalTime?) = when (direction) {
        Direction.OUTBOUND -> copy(outbound = time)
        Direction.INBOUND -> copy(inbound = time)
    }

    companion object {
        /** From `GET /settings`, in the shape fixed by `fixtures/api/settings.json`; none from an older backend. */
        fun parse(json: String): UsualTrains {
            val trains = JSONObject(json).optJSONObject("usual_trains") ?: return UsualTrains(null, null)
            // Android's org.json may read a null as the text "null": check for it first.
            fun time(name: String) = if (trains.isNull(name)) null else LocalTime.parse(trains.getString(name))
            return UsualTrains(time("outbound"), time("inbound"))
        }
    }
}
