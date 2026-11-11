package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test
import java.io.File
import java.time.DayOfWeek
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime

class StatsTest {
    private val day = LocalDate.parse("2026-11-03")
    private val l = Line("L", Mode.TRAIN)
    private val fixture = File("../../fixtures/api/stats.json").readText()

    @Test
    fun readsTheStatisticsOfEachWay() {
        val stats = Stats.parse(fixture)
        val count = Stats.Count(trains = 2, onTime = 1, late = 0, cancelled = 1)
        val (ran, gone) = Stats.Count(1, 1, 0, 0) to Stats.Count(1, 0, 0, 1)
        val tuesday = listOf(Stats.Weekday(DayOfWeek.TUESDAY, 1, count))
        assertEquals(
            Stats(
                since = day,
                ways = listOf(
                    Stats.Way(
                        Status.Direction.OUTBOUND,
                        l,
                        hexColor("#7B61C9"),
                        LocalTime.of(7, 52),
                        count,
                        listOf(Stats.Day(day, count, Stats.UsualRun(Instant.parse("2026-11-03T06:52:00Z"), Stats.Run.CANCELLED, 0))),
                        times = listOf(Stats.Slot(LocalTime.of(7, 52), gone), Stats.Slot(LocalTime.of(8, 7), ran)),
                        weekdays = tuesday,
                    ),
                    Stats.Way(
                        Status.Direction.INBOUND,
                        l,
                        hexColor("#7B61C9"),
                        null,
                        count,
                        listOf(Stats.Day(day, count, null)),
                        times = listOf(Stats.Slot(LocalTime.of(17, 22), ran), Stats.Slot(LocalTime.of(17, 37), gone)),
                        weekdays = tuesday,
                    ),
                ),
                disruptions = Stats.Disruptions(
                    since = day,
                    lines = listOf(
                        Stats.Disrupted(l, hexColor("#7B61C9"), days = 1, alerts = 1),
                        Stats.Disrupted(Line("X", Mode.METRO), hexColor("#E07A1F"), days = 1, alerts = 1),
                    ),
                ),
            ),
            stats,
        )
    }

    @Test
    fun fromABackendBeforeThemThereAreNoTimesWeekdaysOrDisruptions() {
        val older = JSONObject(fixture).apply {
            remove("disruptions")
            val ways = getJSONArray("ways")
            for (i in 0 until ways.length()) {
                ways.getJSONObject(i).apply {
                    remove("times")
                    remove("weekdays")
                }
            }
        }
        val stats = Stats.parse(older.toString())
        assertEquals(null, stats.disruptions)
        assertEquals(listOf(emptyList<Stats.Slot>()), stats.ways.map { it.times }.distinct())
        assertEquals(listOf(emptyList<Stats.Weekday>()), stats.ways.map { it.weekdays }.distinct())
    }

    @Test
    fun withNothingKeptThereIsNothing() {
        assertEquals(
            Stats(null, emptyList(), Stats.Disruptions(null, emptyList())),
            Stats.parse("""{"since": null, "ways": [], "disruptions": {"since": null, "lines": []}}"""),
        )
    }

    @Test
    fun aRunANewerBackendNamesOtherwiseIsTakenAsLate() {
        val text = fixture.replace("\"run\": \"cancelled\"", "\"run\": \"diverted\"")
        assertEquals(Stats.Run.LATE, Stats.parse(text).ways[0].days[0].usual!!.run)
    }

    @Test
    fun theShareOnTimeIsInWholePercentsRoundedDown() {
        assertEquals(66, Stats.Count(3, 2, 1, 0).onTimePercent)
        assertEquals(100, Stats.Count(3, 3, 0, 0).onTimePercent)
        assertEquals(0, Stats.Count(0, 0, 0, 0).onTimePercent)
    }
}
