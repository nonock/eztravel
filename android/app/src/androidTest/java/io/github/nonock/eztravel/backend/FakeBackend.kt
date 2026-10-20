package io.github.nonock.eztravel.backend

import org.json.JSONException
import java.io.IOException
import java.time.LocalDate
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicInteger

/** A backend kept in memory, to test the app without a network. */
class FakeBackend(
    override val configured: Boolean = true,
    var away: LocalDate? = null,
    var failing: Boolean = false,
    /** Answers that cannot be read, as from a server in a bad state. */
    var garbled: Boolean = false,
    /** Outside the windows, with nothing checked yet. */
    var status: Status = Status(null, null, null, null, emptyList(), emptyList()),
    var history: List<HistoryEntry> = emptyList(),
    var departures: Departures = Departures(Status.Direction.OUTBOUND, emptyList()),
    var alerts: Alerts = Alerts(emptyList()),
    /** The answer to picks: null when saved. */
    var refusal: Refusal? = null,
    /** Picks cannot be saved, while all else works. */
    var pickFailing: Boolean = false,
) : Backend {
    val registered = CopyOnWriteArrayList<String>()
    val picks = CopyOnWriteArrayList<String?>()
    val saved = CopyOnWriteArrayList<String>()
    val statusReads = AtomicInteger()
    val departuresReads = AtomicInteger()
    val alertsReads = AtomicInteger()
    val historyReads = AtomicInteger()

    override fun register(token: String) {
        if (failing) throw IOException("unreachable")
        registered += token
    }

    override suspend fun awayThrough(): LocalDate? {
        if (failing) throw IOException("unreachable")
        if (garbled) throw JSONException("not JSON")
        return away
    }

    override suspend fun setAwayThrough(date: LocalDate?) {
        if (failing) throw IOException("unreachable")
        saved += date.toString()
        away = date
    }

    override suspend fun history(): List<HistoryEntry> {
        historyReads.incrementAndGet()
        if (failing) throw IOException("unreachable")
        if (garbled) throw JSONException("not JSON")
        return history
    }

    override suspend fun departures(): Departures {
        departuresReads.incrementAndGet()
        if (failing) throw IOException("unreachable")
        if (garbled) throw JSONException("not JSON")
        return departures
    }

    override suspend fun alerts(): Alerts {
        alertsReads.incrementAndGet()
        if (failing) throw IOException("unreachable")
        if (garbled) throw JSONException("not JSON")
        return alerts
    }

    override suspend fun pick(journey: String?): Refusal? {
        if (failing || pickFailing) throw IOException("unreachable")
        picks += journey
        return refusal
    }

    override suspend fun status(): Status {
        statusReads.incrementAndGet()
        if (failing) throw IOException("unreachable")
        if (garbled) throw JSONException("not JSON")
        return status
    }
}
