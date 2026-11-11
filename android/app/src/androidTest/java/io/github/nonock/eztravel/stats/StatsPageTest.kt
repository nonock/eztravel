package io.github.nonock.eztravel.stats

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.font.FontWeight
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.ScreenTesting
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.backend.Stats
import io.github.nonock.eztravel.backend.Status
import io.github.nonock.eztravel.home.shortDate
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import java.time.DayOfWeek
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.format.TextStyle
import java.util.Locale

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
                times = listOf(Stats.Slot(usual, Stats.Count(3, 2, 0, 1)), Stats.Slot(usual.plusMinutes(15), Stats.Count(3, 3, 0, 0))),
                weekdays = listOf(Stats.Weekday(DayOfWeek.MONDAY, 2, Stats.Count(19, 17, 1, 1)), Stats.Weekday(DayOfWeek.TUESDAY, 1, Stats.Count(1, 0, 1, 0))),
            ),
            Stats.Way(Status.Direction.INBOUND, l, null, null, Stats.Count(10, 10, 0, 0), listOf(Stats.Day(day, Stats.Count(10, 10, 0, 0), null))),
        ),
        disruptions = Stats.Disruptions(
            since = day.minusDays(2),
            lines = listOf(Stats.Disrupted(l, null, days = 2, alerts = 3), Stats.Disrupted(Line("X", Mode.METRO), null, days = 1, alerts = 1)),
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
    fun eachTimeAndWeekdaySaysHowManyOfItsTrainsRanOnTime() {
        compose.setContent { StatsPage(FakeBackend(stats = stats)) }
        waitFor(text(R.string.stats_times))
        val format = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)
        // The usual train's time, then the next.
        compose.onNodeWithText(format.format(usual)).assertExists()
        compose.onNodeWithText(text(R.string.stats_on_time_of, 2, 3)).assertExists()
        compose.onNodeWithText(format.format(usual.plusMinutes(15))).assertExists()
        compose.onNodeWithText(text(R.string.stats_on_time_of, 3, 3)).assertExists()
        compose.onNodeWithText(text(R.string.stats_weekdays)).assertExists()
        val locale = Locale.getDefault()
        val monday = DayOfWeek.MONDAY.getDisplayName(TextStyle.FULL, locale).let { it.substring(0, 1).uppercase(locale) + it.substring(1) }
        val days = context.resources.getQuantityString(R.plurals.stats_days, 2, 2)
        compose.onNodeWithText(text(R.string.stats_weekday, monday, days)).assertExists()
        compose.onNodeWithText(text(R.string.stats_on_time_of, 17, 19)).assertExists()
        // The way home has neither.
        compose.onAllNodesWithText(text(R.string.stats_times)).assertCountEquals(1)
        compose.onAllNodesWithText(text(R.string.stats_weekdays)).assertCountEquals(1)
    }

    /** Whether the text [label] is in bold. */
    private fun bold(label: String): Boolean {
        val node = compose.onNodeWithText(label, useUnmergedTree = true).fetchSemanticsNode()
        val layouts = mutableListOf<TextLayoutResult>()
        node.config[SemanticsActions.GetTextLayoutResult].action!!.invoke(layouts)
        return layouts.single().layoutInput.style.fontWeight == FontWeight.Bold
    }

    @Test
    fun theUsualTrainsTimeIsTheFirstFromTheTimeSetAsEachDay() {
        val count = Stats.Count(1, 1, 0, 0)
        fun way(direction: Status.Direction, line: Line, usual: LocalTime?, vararg times: LocalTime) =
            Stats.Way(direction, line, null, usual, count, emptyList(), times = times.map { Stats.Slot(it, count) })
        val at = { hour: Int, minute: Int -> LocalTime.of(hour, minute) }
        val ways = listOf(
            // Set between two trains: the next one.
            way(Status.Direction.OUTBOUND, l, at(7, 50), at(7, 46), at(7, 52), at(8, 7)),
            // Set after the last train, or not set: none.
            way(Status.Direction.INBOUND, l, at(19, 0), at(17, 52)),
            way(Status.Direction.INBOUND, Line("X", Mode.METRO), null, at(17, 40)),
        )
        compose.setContent { StatsPage(FakeBackend(stats = Stats(day, ways))) }
        waitFor(text(R.string.usual_outbound))
        val format = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)
        val times = listOf(at(7, 46), at(7, 52), at(8, 7), at(17, 52), at(17, 40))
        assertEquals(listOf(false, true, false, false, false), times.map { bold(format.format(it)) })
    }

    @Test
    fun eachLineToldDisruptedSaysOnHowManyDaysAndWithHowManyAlerts() {
        compose.setContent { StatsPage(FakeBackend(stats = stats)) }
        waitFor(text(R.string.stats_disruptions))
        val resources = context.resources
        compose.onNodeWithText(text(R.string.stats_since, day.minusDays(2).shortDate())).assertExists()
        val disrupted = { days: Int, alerts: Int ->
            text(
                R.string.stats_disrupted,
                resources.getQuantityString(R.plurals.stats_days_disrupted, days, days),
                resources.getQuantityString(R.plurals.stats_alerts, alerts, alerts),
            )
        }
        compose.onNodeWithText(disrupted(2, 3)).assertExists()
        compose.onNodeWithText(disrupted(1, 1)).assertExists()
    }

    @Test
    fun withNoLineDisruptedItSaysSo() {
        compose.setContent { StatsPage(FakeBackend(stats = stats.copy(disruptions = Stats.Disruptions(null, emptyList())))) }
        waitFor(text(R.string.stats_disruptions_none))
    }

    @Test
    fun aBackendBeforeTheDisruptionsShowsNone() {
        compose.setContent { StatsPage(FakeBackend(stats = stats.copy(disruptions = null))) }
        waitFor(text(R.string.usual_outbound))
        compose.onNodeWithText(text(R.string.stats_disruptions)).assertDoesNotExist()
    }

    @Test
    fun withoutAFirstDayItSaysNoneSince() {
        compose.setContent { StatsPage(FakeBackend(stats = stats.copy(since = null))) }
        waitFor(text(R.string.usual_outbound))
        compose.onNodeWithText(text(R.string.stats_since, day.minusDays(1).shortDate())).assertDoesNotExist()
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
