package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import org.junit.Assert.assertEquals
import org.junit.Test
import java.io.File
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime

class StatsTest {
    private val day = LocalDate.parse("2026-11-03")
    private val l = Line("L", Mode.TRAIN)

    @Test
    fun readsTheStatisticsOfEachWay() {
        val stats = Stats.parse(File("../../fixtures/api/stats.json").readText())
        val count = Stats.Count(trains = 2, onTime = 1, late = 0, cancelled = 1)
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
                    ),
                    Stats.Way(Status.Direction.INBOUND, l, hexColor("#7B61C9"), null, count, listOf(Stats.Day(day, count, null))),
                ),
            ),
            stats,
        )
    }

    @Test
    fun withNothingKeptThereIsNothing() {
        assertEquals(Stats(null, emptyList()), Stats.parse("""{"since": null, "ways": []}"""))
    }

    @Test
    fun aRunANewerBackendNamesOtherwiseIsTakenAsLate() {
        val text = File("../../fixtures/api/stats.json").readText().replace("\"run\": \"cancelled\"", "\"run\": \"diverted\"")
        assertEquals(Stats.Run.LATE, Stats.parse(text).ways[0].days[0].usual!!.run)
    }

    @Test
    fun theShareOnTimeIsInWholePercentsRoundedDown() {
        assertEquals(66, Stats.Count(3, 2, 1, 0).onTimePercent)
        assertEquals(100, Stats.Count(3, 3, 0, 0).onTimePercent)
        assertEquals(0, Stats.Count(0, 0, 0, 0).onTimePercent)
    }
}
