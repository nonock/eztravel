package io.github.nonock.eztravel.backend

import org.json.JSONException
import java.io.IOException
import java.time.DayOfWeek
import java.time.LocalDate
import java.time.LocalTime
import java.time.YearMonth
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicInteger

/** A backend kept in memory, to test the app without a network. */
class FakeBackend(
    override val configured: Boolean = true,
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
    var usual: UsualTrains = UsualTrains(null, null),
    /** The answer to setting a usual train: null when saved. */
    var usualRefusal: Refusal? = null,
    /** What each day is planned to be: by default, office from Monday to Wednesday, at home Thursday and Friday. */
    var plan: (LocalDate) -> CalendarDay = ::plannedWeekdays,
    /** The answer to setting a day: null when saved. */
    var dayRefusal: DayRefusal? = null,
    /** While set, months are read once it completes, as from a slow server. */
    var monthGate: kotlinx.coroutines.CompletableDeferred<Unit>? = null,
    /** While set, the status is read once it completes. */
    var statusGate: kotlinx.coroutines.CompletableDeferred<Unit>? = null,
) : Backend {
    /** Days set otherwise than planned, as the backend keeps them. */
    val set = java.util.concurrent.ConcurrentHashMap<LocalDate, CalendarDay>()
    /** Each change: its first day, its last, and what they are set to. */
    val daysSet = CopyOnWriteArrayList<Triple<LocalDate, LocalDate, CalendarDay.Kind?>>()
    val monthReads = CopyOnWriteArrayList<YearMonth>()

    val registered = CopyOnWriteArrayList<String>()
    val picks = CopyOnWriteArrayList<String?>()
    val usualSaved = CopyOnWriteArrayList<Pair<Status.Direction, LocalTime?>>()
    val statusReads = AtomicInteger()
    /** Status reads given up before their answer, as when the look is stopped. */
    val statusDropped = AtomicInteger()
    val departuresReads = AtomicInteger()
    val alertsReads = AtomicInteger()
    val historyReads = AtomicInteger()

    override fun register(token: String) {
        if (failing) throw IOException("unreachable")
        registered += token
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

    override suspend fun usualTrains(): UsualTrains {
        if (failing) throw IOException("unreachable")
        if (garbled) throw JSONException("not JSON")
        return usual
    }

    override suspend fun setUsualTrain(direction: Status.Direction, time: LocalTime?): Refusal? {
        if (failing) throw IOException("unreachable")
        usualSaved += direction to time
        if (usualRefusal == null) usual = usual.with(direction, time)
        return usualRefusal
    }

    override suspend fun month(month: YearMonth): List<CalendarDay> {
        monthReads += month
        monthGate?.await()
        if (failing) throw IOException("unreachable")
        if (garbled) throw JSONException("not JSON")
        return (1..month.lengthOfMonth()).map(month::atDay).map { set[it] ?: plan(it) }
    }

    override suspend fun setDays(first: LocalDate, last: LocalDate, kind: CalendarDay.Kind?): DayRefusal? {
        if (failing) throw IOException("unreachable")
        daysSet += Triple(first, last, kind)
        if (dayRefusal != null) return dayRefusal
        for (date in generateSequence(first) { it.plusDays(1) }.takeWhile { it <= last }) {
            // As the backend: a day set as planned is not kept.
            if (kind == null || kind == plan(date).kind) set.remove(date) else set[date] = CalendarDay(date, kind, CalendarDay.Why.SET)
        }
        return null
    }

    override suspend fun status(): Status {
        statusReads.incrementAndGet()
        try {
            statusGate?.await()
        } catch (stopped: kotlinx.coroutines.CancellationException) {
            statusDropped.incrementAndGet()
            throw stopped
        }
        if (failing) throw IOException("unreachable")
        if (garbled) throw JSONException("not JSON")
        return status
    }
}

/** Office from Monday to Wednesday, at home Thursday and Friday, weekends off. */
fun plannedWeekdays(date: LocalDate): CalendarDay {
    val kind = when (date.dayOfWeek) {
        DayOfWeek.MONDAY, DayOfWeek.TUESDAY, DayOfWeek.WEDNESDAY -> CalendarDay.Kind.OFFICE
        DayOfWeek.THURSDAY, DayOfWeek.FRIDAY -> CalendarDay.Kind.REMOTE
        else -> CalendarDay.Kind.OFF
    }
    return CalendarDay(date, kind, CalendarDay.Why.CYCLE)
}
