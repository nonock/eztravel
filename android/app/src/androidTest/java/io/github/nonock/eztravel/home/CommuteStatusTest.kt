package io.github.nonock.eztravel.home

import io.github.nonock.eztravel.ScreenTesting
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Status
import io.github.nonock.eztravel.backend.Status.Cause
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.backend.Status.LineState
import io.github.nonock.eztravel.backend.Status.Outage
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.util.concurrent.atomic.AtomicInteger

@RunWith(AndroidJUnit4::class)
class CommuteStatusTest : ScreenTesting() {
    private val now = Instant.now()
    private val train = Line("L", Mode.TRAIN)
    private val metro = Line("X", Mode.METRO)
    private val taps = AtomicInteger()

    /** What the screen was given, changed from one case to the next: content is set once a test. */
    private var reading by mutableStateOf<Reading<Status>>(Reading.Loading)

    private fun watching(direction: Direction, vararg lines: LineState, planned: List<String> = emptyList(), outage: Outage? = null) =
        Status(direction, now, now.plus(Duration.ofHours(8)), outage, lines.toList(), planned)

    private val fine = watching(Direction.OUTBOUND, LineState(train, null), LineState(metro, null))

    /** What the gear's neighbour opens: all the backend sees. */
    private fun details(given: Reading<Status>) {
        reading = given
        compose.setContent { StatusDetails(reading, now) }
    }

    private fun details(status: Status) = details(Reading.Known(status))

    /** A line's row: its badge names the line, its text says how it runs. */
    private fun row(line: String, state: Int) = hasContentDescription(line) and hasText(text(state))

    @Test
    fun whileWatchingEachLineAndTheLastCheckAreShown() {
        details(
            watching(
                Direction.OUTBOUND,
                LineState(train, Severity.BLOCKING, color = 0xFF7B61C9.toInt()),
                LineState(metro, null),
                planned = listOf("Ligne L : travaux entre Nanterre et Cergy"),
            ),
        )
        waitFor(text(R.string.status_watching_outbound))
        waitFor(row(text(R.string.line_train, "L"), R.string.status_state_blocking))
        waitFor(row(text(R.string.line_metro, "X"), R.string.status_state_normal))
        waitFor(text(R.string.status_checked, now.shown()))
        waitFor(text(R.string.status_planned))
        waitFor("Ligne L : travaux entre Nanterre et Cergy")
        compose.onNodeWithText(text(R.string.status_not_watching)).assertDoesNotExist()
    }

    @Test
    fun theHeadlineSaysAllIsFineOnlyWhenItIsKnown() {
        details(fine)
        waitFor(text(R.string.status_fine))
        // Every line, with how it runs: there is room for it here.
        waitFor(row(text(R.string.line_train, "L"), R.string.status_state_normal))
    }

    @Test
    fun theHeadlineNamesTheLineInTrouble() {
        details(watching(Direction.INBOUND, LineState(metro, Severity.DISRUPTED), LineState(train, Severity.BLOCKING)))
        waitFor(text(R.string.status_headline_blocking, text(R.string.line_train, "L")))
        compose.onNodeWithText(text(R.string.status_fine)).assertDoesNotExist()
    }

    @Test
    fun aLineSlowedIsTheHeadlineWhenNoneIsStopped() {
        details(watching(Direction.INBOUND, LineState(metro, Severity.DISRUPTED)))
        waitFor(text(R.string.status_headline_disrupted, text(R.string.line_metro, "X")))
    }

    @Test
    fun aCheckNotYetMadeIsNoGoodNews() {
        details(Status(Direction.OUTBOUND, now.minus(Duration.ofMinutes(20)), null, null, listOf(LineState(train, null)), emptyList()))
        waitFor(text(R.string.status_checking))
        compose.onNodeWithText(text(R.string.status_fine)).assertDoesNotExist()
    }

    @Test
    fun theWayHomeIsSaidToo() {
        details(watching(Direction.INBOUND, LineState(metro, Severity.DISRUPTED)))
        waitFor(text(R.string.status_watching_inbound))
        waitFor(row(text(R.string.line_metro, "X"), R.string.status_state_disrupted))
        compose.onNodeWithText(text(R.string.status_planned)).assertDoesNotExist()
    }

    @Test
    fun outsideTheWindowsTheNextWatchAndTheLastCheckAreShown() {
        val yesterday = now.minus(Duration.ofDays(1))
        val tomorrow = now.plus(Duration.ofDays(1))
        details(Status(null, yesterday, tomorrow, null, emptyList(), emptyList()))
        waitFor(text(R.string.status_not_watching))
        compose.onNodeWithText(text(R.string.status_fine)).assertDoesNotExist()
        waitFor(text(R.string.status_next, tomorrow.shown()))
        waitFor(text(R.string.status_checked, yesterday.shown()))
    }

    @Test
    fun withoutAnOfficeDayAheadItSaysSo() {
        details(Status(null, null, null, null, emptyList(), emptyList()))
        waitFor(text(R.string.status_no_next))
        // Nothing checked yet: no "last check" line at all.
        val lastCheck = text(R.string.status_checked, "@").substringBefore("@")
        compose.onNodeWithText(lastCheck, substring = true).assertDoesNotExist()
    }

    @Test
    fun anOutageIsSaidWithItsCause() {
        val since = now.minus(Duration.ofMinutes(12))
        details(Reading.Loading)
        for ((cause, words) in listOf(
            Cause.PRIM to R.string.status_outage_prim,
            Cause.FIREBASE to R.string.status_outage_firebase,
            Cause.SERVER to R.string.status_outage_server,
        )) {
            reading = Reading.Known(watching(Direction.OUTBOUND, outage = Outage(since, cause)))
            waitFor(text(R.string.status_headline_outage))
            waitFor(text(words, since.shown()))
        }
    }

    @Test
    fun anUnreachableServerIsSaid() {
        details(Reading.Failed)
        waitFor(text(R.string.status_unreachable))
        waitFor(text(R.string.status_headline_unreachable))
    }

    @Test
    fun whileReadingItSaysSo() {
        details(Reading.Loading)
        waitFor(text(R.string.loading))
    }

    /** The Toussaint 2026, from Saturday 17 October through Sunday 1 November. */
    private val toussaint = fine.copy(
        schoolHoliday = "Vacances de la Toussaint",
        schoolHolidayFirst = LocalDate.of(2026, 10, 17),
        schoolHolidayLast = LocalDate.of(2026, 11, 1),
    )

    /** "Vacances de la Toussaint, du 17 oct. au 1 nov.": what they are and how long. */
    private val toussaintWords = text(
        R.string.status_school_holiday,
        "Vacances de la Toussaint",
        LocalDate.of(2026, 10, 17).shortDate(),
        LocalDate.of(2026, 11, 1).shortDate(),
    )

    @Test
    fun theSchoolHolidaysAreSaidOnTheirDays() {
        details(toussaint)
        waitFor(toussaintWords)
        // From a backend that gives their name only.
        reading = Reading.Known(toussaint.copy(schoolHolidayFirst = null, schoolHolidayLast = null))
        waitFor("Vacances de la Toussaint")
    }

    /** The icon beside the gear: named by the headline, it opens the details. */
    private fun icon(given: Reading<Status>) {
        reading = given
        compose.setContent { StatusIcon(reading, now) { taps.incrementAndGet() } }
    }

    @Test
    fun theIconSaysTheHeadlineAndOpensTheDetails() {
        icon(Reading.Known(fine))
        compose.onNodeWithContentDescription(text(R.string.status_fine)).performClick()
        assertEquals(1, taps.get())
    }

    @Test
    fun theIconHasAWordForEveryState() {
        val idle = Status(null, null, null, null, emptyList(), emptyList())
        icon(Reading.Loading)
        for ((each, words) in listOf(
            Reading.Loading to text(R.string.loading),
            Reading.Failed to text(R.string.status_headline_unreachable),
            Reading.Known(idle) to text(R.string.status_not_watching),
            Reading.Known(watching(Direction.OUTBOUND, LineState(train, Severity.DISRUPTED))) to
                text(R.string.status_headline_disrupted, text(R.string.line_train, "L")),
        )) {
            reading = each
            compose.onNodeWithContentDescription(words).assertIsDisplayed()
        }
    }

    /** On the commute, above the trains: only what goes wrong. */
    private fun banner(given: Reading<Status>) {
        reading = given
        compose.setContent { StatusBanner(reading, now) { taps.incrementAndGet() } }
    }

    @Test
    fun aBannerSaysWhatGoesWrongAndOpensTheDetails() {
        banner(Reading.Known(watching(Direction.OUTBOUND, LineState(metro, Severity.BLOCKING))))
        compose.onNodeWithText(text(R.string.status_headline_blocking, text(R.string.line_metro, "X"))).performClick()
        assertEquals(1, taps.get())
        val since = now.minus(Duration.ofMinutes(12))
        reading = Reading.Known(watching(Direction.OUTBOUND, outage = Outage(since, Cause.PRIM)))
        waitFor(text(R.string.status_headline_outage))
        reading = Reading.Failed
        waitFor(text(R.string.status_headline_unreachable))
    }

    @Test
    fun noBannerWhileAllIsWell() {
        val idle = Status(null, null, null, null, emptyList(), emptyList())
        banner(Reading.Loading)
        for (each in listOf(Reading.Loading, Reading.Known(fine), Reading.Known(idle))) {
            reading = each
            compose.waitForIdle()
            for (words in listOf(R.string.status_fine, R.string.status_not_watching, R.string.loading)) {
                compose.onNodeWithText(text(words)).assertDoesNotExist()
            }
        }
    }

    @Test
    fun theSchoolHolidaysHaveALineOnTheCommute() {
        compose.setContent { SchoolHoliday(Reading.Known(toussaint)) }
        waitFor(toussaintWords)
    }

    @Test
    fun noSchoolHolidayLineOtherwise() {
        compose.setContent { SchoolHoliday(Reading.Known(fine)) }
        compose.waitForIdle()
        compose.onNodeWithText("Vacances", substring = true).assertDoesNotExist()
    }
}
