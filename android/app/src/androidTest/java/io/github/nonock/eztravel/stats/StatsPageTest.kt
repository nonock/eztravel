package io.github.nonock.eztravel.stats

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.ScreenTesting
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.backend.Stats
import io.github.nonock.eztravel.backend.Status
import io.github.nonock.eztravel.home.shortDate
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import org.junit.Test
import org.junit.runner.RunWith
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

@RunWith(AndroidJUnit4::class)
class StatsPageTest : ScreenTesting() {
    private val l = Line("L", Mode.TRAIN)
    private val day = LocalDate.of(2026, 11, 3)
    private val usual = LocalTime.of(7, 52)

    /** The usual train of [date], leaving at its time in the phone's zone. */
    private fun usualOf(date: LocalDate, run: Stats.Run, late: Int) =
        Stats.UsualRun(date.atTime(usual).atZone(ZoneId.systemDefault()).toInstant(), run, late)

    private val stats = Stats(
        since = day.minusDays(1),
        ways = listOf(
            Stats.Way(
                Status.Direction.OUTBOUND,
                l,
                null,
                usual,
                Stats.Count(trains = 20, onTime = 17, late = 2, cancelled = 1),
                listOf(
                    Stats.Day(day.plusDays(1), Stats.Count(10, 9, 1, 0), usualOf(day.plusDays(1), Stats.Run.LATE, 6)),
                    Stats.Day(day, Stats.Count(9, 8, 0, 1), usualOf(day, Stats.Run.CANCELLED, 0)),
                    Stats.Day(day.minusDays(1), Stats.Count(1, 0, 1, 0), null),
                ),
            ),
            Stats.Way(Status.Direction.INBOUND, l, null, null, Stats.Count(10, 10, 0, 0), listOf(Stats.Day(day, Stats.Count(10, 10, 0, 0), null))),
        ),
    )

    @Test
    fun eachWaySaysHowManyRanOnTimeAndHowItsUsualTrainDidEachDay() {
        compose.setContent { StatsPage(FakeBackend(stats = stats)) }
        waitFor(text(R.string.stats_on_time_share, 85))
        compose.onNodeWithText(text(R.string.stats_since, day.minusDays(1).shortDate())).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.usual_outbound)).assertIsDisplayed()
        val resources = context.resources
        val counts = text(
            R.string.stats_count,
            resources.getQuantityString(R.plurals.stats_trains, 20, 20),
            17,
            2,
            resources.getQuantityString(R.plurals.stats_cancelled, 1, 1),
        )
        compose.onNodeWithText(counts).assertIsDisplayed()
        val time = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).format(usual)
        compose.onNodeWithText(text(R.string.stats_usual, time)).assertIsDisplayed()
        compose.onNodeWithText(day.plusDays(1).shortDate()).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.stats_run_late, 6)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.stats_run_cancelled)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.stats_run_none)).assertExists()
        // The way home: all on time, and no usual train set.
        compose.onNodeWithText(text(R.string.stats_on_time_share, 100)).assertExists()
        compose.onNodeWithText(text(R.string.usual_inbound)).assertExists()
        compose.onAllNodesWithText(text(R.string.stats_usual, time)).assertCountEquals(1)
    }

    @Test
    fun aUsualTrainOnTimeSaysSo() {
        val onTime = stats.copy(ways = listOf(stats.ways[0].copy(days = listOf(Stats.Day(day, Stats.Count(1, 1, 0, 0), usualOf(day, Stats.Run.ON_TIME, 1))))))
        compose.setContent { StatsPage(FakeBackend(stats = onTime)) }
        waitFor(text(R.string.stats_run_on_time))
    }

    @Test
    fun withNoTrainFollowedYetItSaysSo() {
        compose.setContent { StatsPage(FakeBackend(stats = Stats(null, emptyList()))) }
        waitFor(text(R.string.stats_empty))
    }

    @Test
    fun anUnreachableOrGarbledServerSaysSo() {
        compose.setContent { StatsPage(FakeBackend(failing = true)) }
        waitFor(text(R.string.unreachable))
    }

    @Test
    fun aGarbledAnswerSaysSoToo() {
        compose.setContent { StatsPage(FakeBackend(garbled = true)) }
        waitFor(text(R.string.unreachable))
    }
}
