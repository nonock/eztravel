package io.github.nonock.eztravel.home

import io.github.nonock.eztravel.ScreenTesting
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithText
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.FakeBackend
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

@RunWith(AndroidJUnit4::class)
class CommuteStatusTest : ScreenTesting() {
    private var refresh by mutableIntStateOf(0)

    private fun show(backend: FakeBackend) = compose.setContent { CommuteStatus(backend, refresh) }

    /** A line's row: its badge names the line, its text says how it runs. */
    private fun row(line: String, state: Int) = hasContentDescription(line) and hasText(text(state))

    private val now = Instant.now()
    private val train = Line("L", Mode.TRAIN)
    private val metro = Line("X", Mode.METRO)

    private fun watching(direction: Direction, vararg lines: LineState, planned: List<String> = emptyList(), outage: Outage? = null) =
        Status(direction, now, now.plus(Duration.ofHours(8)), outage, lines.toList(), planned)

    @Test
    fun theSchoolHolidaysAreSaidOnTheirDays() {
        val backend = FakeBackend(status = watching(Direction.OUTBOUND, LineState(train, null)))
        show(backend)
        waitFor(text(R.string.status_fine))
        compose.onNodeWithText(text(R.string.status_school_holiday, "Vacances de la Toussaint")).assertDoesNotExist()
        backend.status = backend.status.copy(schoolHoliday = "Vacances de la Toussaint")
        refresh++
        waitFor(text(R.string.status_school_holiday, "Vacances de la Toussaint"))
    }

    @Test
    fun whileWatchingEachLineAndTheLastCheckAreShown() {
        show(
            FakeBackend(
                status = watching(
                    Direction.OUTBOUND,
                    LineState(train, Severity.BLOCKING, color = 0xFF7B61C9.toInt()),
                    LineState(metro, null),
                    planned = listOf("Ligne L : travaux entre Nanterre et Cergy"),
                ),
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
    fun whenAllRunsTheCardIsShort() {
        show(FakeBackend(status = watching(Direction.OUTBOUND, LineState(train, null), LineState(metro, null))))
        waitFor(text(R.string.status_fine))
        // The lines by their badges, and when they were checked: the headline says how they run.
        waitFor(hasContentDescription(text(R.string.line_train, "L")))
        waitFor(hasContentDescription(text(R.string.line_metro, "X")))
        waitFor(text(R.string.status_checked, now.shown()))
        compose.onNodeWithText(text(R.string.status_state_normal)).assertDoesNotExist()
        compose.onNodeWithText(text(R.string.status_watching_outbound)).assertDoesNotExist()
    }

    @Test
    fun theHeadlineSaysAllIsFineOnlyWhenItIsKnown() {
        show(FakeBackend(status = watching(Direction.OUTBOUND, LineState(train, null), LineState(metro, null))))
        waitFor(text(R.string.status_fine))
    }

    @Test
    fun theHeadlineNamesTheLineInTrouble() {
        show(FakeBackend(status = watching(Direction.INBOUND, LineState(metro, Severity.DISRUPTED), LineState(train, Severity.BLOCKING))))
        waitFor(text(R.string.status_headline_blocking, text(R.string.line_train, "L")))
        compose.onNodeWithText(text(R.string.status_fine)).assertDoesNotExist()
    }

    @Test
    fun aLineSlowedIsTheHeadlineWhenNoneIsStopped() {
        show(FakeBackend(status = watching(Direction.INBOUND, LineState(metro, Severity.DISRUPTED))))
        waitFor(text(R.string.status_headline_disrupted, text(R.string.line_metro, "X")))
    }

    @Test
    fun aCheckNotYetMadeIsNoGoodNews() {
        val stale = Status(Direction.OUTBOUND, now.minus(Duration.ofMinutes(20)), null, null, listOf(LineState(train, null)), emptyList())
        show(FakeBackend(status = stale))
        waitFor(text(R.string.status_checking))
        compose.onNodeWithText(text(R.string.status_fine)).assertDoesNotExist()
    }

    @Test
    fun theWayHomeIsSaidToo() {
        show(FakeBackend(status = watching(Direction.INBOUND, LineState(metro, Severity.DISRUPTED))))
        waitFor(text(R.string.status_watching_inbound))
        waitFor(row(text(R.string.line_metro, "X"), R.string.status_state_disrupted))
        compose.onNodeWithText(text(R.string.status_planned)).assertDoesNotExist()
    }

    @Test
    fun outsideTheWindowsTheNextWatchIsShown() {
        val yesterday = now.minus(Duration.ofDays(1))
        val tomorrow = now.plus(Duration.ofDays(1))
        show(FakeBackend(status = Status(null, yesterday, tomorrow, null, emptyList(), emptyList())))
        waitFor(text(R.string.status_not_watching))
        compose.onNodeWithText(text(R.string.status_fine)).assertDoesNotExist()
        waitFor(text(R.string.status_next, tomorrow.shown()))
        // Outside the windows, the last check vouches for nothing.
        val lastCheck = text(R.string.status_checked, "@").substringBefore("@")
        compose.onNodeWithText(lastCheck, substring = true).assertDoesNotExist()
    }

    @Test
    fun withoutAnOfficeDayAheadItSaysSo() {
        show(FakeBackend())
        waitFor(text(R.string.status_no_next))
        // Nothing checked yet: no "last check" line at all.
        val lastCheck = text(R.string.status_checked, "@").substringBefore("@")
        compose.onNodeWithText(lastCheck, substring = true).assertDoesNotExist()
    }

    @Test
    fun anOutageIsSaidWithItsCauseAndReadAgainWhenAsked() {
        val since = now.minus(Duration.ofMinutes(12))
        val backend = FakeBackend(status = watching(Direction.OUTBOUND, outage = Outage(since, Cause.PRIM)))
        show(backend)
        waitFor(text(R.string.status_outage_prim, since.shown()))
        waitFor(text(R.string.status_headline_outage))
        backend.status = watching(Direction.OUTBOUND, outage = Outage(since, Cause.FIREBASE))
        refresh++
        waitFor(text(R.string.status_outage_firebase, since.shown()))
        backend.status = Status(null, null, null, Outage(since, Cause.SERVER), emptyList(), emptyList())
        refresh++
        waitFor(text(R.string.status_outage_server, since.shown()))
        assertEquals(3, backend.statusReads.get())
    }

    @Test
    fun anUnreachableServerIsSaid() {
        show(FakeBackend(failing = true))
        waitFor(text(R.string.status_unreachable))
        waitFor(text(R.string.status_headline_unreachable))
    }

    @Test
    fun aGarbledAnswerIsTakenAsAnUnreachableServer() {
        show(FakeBackend(garbled = true))
        waitFor(text(R.string.status_unreachable))
    }

    @Test
    fun aBuildWithoutServerLeavesItToTheCommuteTab() {
        show(FakeBackend(configured = false))
        compose.waitForIdle()
        compose.onNodeWithText(text(R.string.status_no_next)).assertDoesNotExist()
    }
}
