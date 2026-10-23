package io.github.nonock.eztravel.calendar

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.assertHasNoClickAction
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.CalendarDay
import io.github.nonock.eztravel.backend.CalendarDay.Kind
import io.github.nonock.eztravel.backend.CalendarDay.Why
import io.github.nonock.eztravel.backend.DayRefusal
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.backend.plannedWeekdays
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.time.LocalDate
import java.time.YearMonth
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.concurrent.atomic.AtomicInteger

@RunWith(AndroidJUnit4::class)
class CalendarPageTest {
    @get:Rule
    val compose = createComposeRule()

    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    private fun text(id: Int, vararg args: Any) = context.getString(id, *args)

    /** Wednesday 4 November 2026: the Sunday, Monday and Tuesday before are gone by. */
    private val today = LocalDate.of(2026, 11, 4)
    private val november = YearMonth.of(2026, 11)
    private val saves = AtomicInteger()

    private fun show(backend: FakeBackend) =
        compose.setContent { CalendarPage(backend, today = today, onSaved = { saves.incrementAndGet() }) }

    private fun day(day: Int) = november.atDay(day)

    /** A day of the grid, by what it tells a screen reader: its date, what it is, whether set, whether today. */
    private fun cell(date: LocalDate, what: Int, set: Boolean = false) = compose.onNodeWithContentDescription(
        listOfNotNull(
            DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL).format(date),
            text(what),
            text(R.string.calendar_set).takeIf { set },
            text(R.string.calendar_today).takeIf { date == today },
        ).joinToString(", "),
    )

    /** Waits for the day to show as said, after a change. */
    private fun waitForCell(date: LocalDate, what: Int, set: Boolean = false) {
        compose.waitUntil(timeoutMillis = 5_000) { runCatching { cell(date, what, set).assertExists() }.isSuccess }
    }

    /** An entry of a day's menu. */
    private fun choice(id: Int) = compose.onNode(hasText(text(id)) and hasClickAction())

    @Test
    fun eachDayOfTheMonthSaysWhatItIs() {
        val backend = FakeBackend(plan = { if (it == day(11)) CalendarDay(it, Kind.OFF, Why.HOLIDAY) else plannedWeekdays(it) })
        backend.set[day(20)] = CalendarDay(day(20), Kind.AWAY, Why.SET)
        show(backend)
        compose.onNodeWithText(monthTitle(november)).assertIsDisplayed()
        cell(day(4), R.string.calendar_office).assertIsDisplayed()
        cell(day(5), R.string.calendar_remote).assertExists()
        cell(day(7), R.string.calendar_weekend).assertExists()
        cell(day(11), R.string.calendar_holiday).assertExists()
        cell(day(20), R.string.calendar_away, set = true).assertExists()
        cell(day(30), R.string.calendar_office).assertExists()
        assertEquals(listOf(november), backend.monthReads)
    }

    @Test
    fun theLegendNamesEachColor() {
        show(FakeBackend())
        for (label in listOf(R.string.calendar_office, R.string.calendar_remote, R.string.calendar_away, R.string.calendar_holiday)) {
            compose.onNodeWithText(text(label)).assertIsDisplayed()
        }
        compose.onNodeWithText(text(R.string.calendar_set_legend)).assertIsDisplayed()
    }

    @Test
    fun daysGoneByCannotBeChanged() {
        show(FakeBackend())
        cell(day(1), R.string.calendar_weekend).assertHasNoClickAction()
        cell(day(3), R.string.calendar_office).assertHasNoClickAction()
        cell(day(4), R.string.calendar_office).assertHasClickAction()
    }

    @Test
    fun aDayIsSetFromItsMenuThenPutBackAsPlanned() {
        val backend = FakeBackend()
        show(backend)
        cell(day(5), R.string.calendar_remote).performClick()
        // The day's own kind is marked, and there is nothing to put back.
        choice(R.string.calendar_remote).assertIsSelected()
        choice(R.string.calendar_office).assertIsNotSelected()
        choice(R.string.calendar_as_planned).assertDoesNotExist()
        choice(R.string.calendar_office).performClick()
        waitForCell(day(5), R.string.calendar_office, set = true)
        assertEquals(listOf(day(5) to Kind.OFFICE), backend.daysSet.toList())
        assertEquals(1, saves.get())
        cell(day(5), R.string.calendar_office, set = true).performClick()
        choice(R.string.calendar_as_planned).performClick()
        waitForCell(day(5), R.string.calendar_remote)
        assertEquals(day(5) to null, backend.daysSet.last())
        assertEquals(2, saves.get())
    }

    @Test
    fun aDayOffIsSetForASingleDay() {
        val backend = FakeBackend()
        show(backend)
        cell(day(4), R.string.calendar_office).performClick()
        choice(R.string.calendar_away).performClick()
        waitForCell(day(4), R.string.calendar_away, set = true)
        assertEquals(listOf(day(4) to Kind.AWAY), backend.daysSet.toList())
    }

    @Test
    fun choosingWhatTheDayIsAlreadyChangesNothing() {
        val backend = FakeBackend()
        show(backend)
        cell(day(5), R.string.calendar_remote).performClick()
        choice(R.string.calendar_remote).performClick()
        choice(R.string.calendar_office).assertDoesNotExist()
        compose.waitForIdle()
        assertTrue(backend.daysSet.isEmpty())
        assertEquals(0, saves.get())
    }

    @Test
    fun theMonthsGoFromThisOneToElevenAhead() {
        val backend = FakeBackend()
        show(backend)
        val previous = compose.onNodeWithContentDescription(text(R.string.calendar_previous))
        val next = compose.onNodeWithContentDescription(text(R.string.calendar_next))
        previous.assertIsNotEnabled()
        repeat(11) { next.performClick() }
        compose.onNodeWithText(monthTitle(YearMonth.of(2027, 10))).assertIsDisplayed()
        next.assertIsNotEnabled()
        previous.performClick()
        compose.onNodeWithText(monthTitle(YearMonth.of(2027, 9))).assertIsDisplayed()
        // Every day of a month ahead can be changed.
        cell(LocalDate.of(2027, 9, 1), R.string.calendar_office).assertHasClickAction()
        compose.waitForIdle()
        assertEquals(YearMonth.of(2027, 9), backend.monthReads.last())
        assertTrue(YearMonth.of(2027, 10) in backend.monthReads)
    }

    @Test
    fun whileAMonthIsReadThePreviousOnesDaysAreNotShown() {
        val backend = FakeBackend()
        show(backend)
        cell(day(5), R.string.calendar_remote).assertExists()
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>()
        backend.monthGate = gate
        compose.onNodeWithContentDescription(text(R.string.calendar_next)).performClick()
        compose.onNodeWithText(monthTitle(YearMonth.of(2026, 12))).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.away_loading)).assertIsDisplayed()
        cell(day(5), R.string.calendar_remote).assertDoesNotExist()
        gate.complete(Unit)
        waitForCell(LocalDate.of(2026, 12, 3), R.string.calendar_remote)
        compose.onNodeWithText(text(R.string.away_loading)).assertDoesNotExist()
    }

    @Test
    fun aDayThatCannotBeSetSaysWhy() {
        val backend = FakeBackend(dayRefusal = DayRefusal.PAST)
        show(backend)
        cell(day(5), R.string.calendar_remote).performClick()
        choice(R.string.calendar_office).performClick()
        compose.onNodeWithText(text(R.string.calendar_past)).assertIsDisplayed()
        cell(day(5), R.string.calendar_remote).assertExists()
        backend.dayRefusal = DayRefusal.TOO_FAR
        cell(day(5), R.string.calendar_remote).performClick()
        choice(R.string.calendar_office).performClick()
        compose.onNodeWithText(text(R.string.calendar_too_far)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.calendar_past)).assertDoesNotExist()
        assertEquals(0, saves.get())
        // Gone once a day is set.
        backend.dayRefusal = null
        cell(day(5), R.string.calendar_remote).performClick()
        choice(R.string.calendar_office).performClick()
        waitForCell(day(5), R.string.calendar_office, set = true)
        compose.onNodeWithText(text(R.string.calendar_too_far)).assertDoesNotExist()
    }

    @Test
    fun aServerOutOfReachIsSaidSo() {
        show(FakeBackend(failing = true))
        compose.onNodeWithText(text(R.string.calendar_unreachable)).assertIsDisplayed()
    }

    @Test
    fun anAnswerThatCannotBeReadIsSaidSo() {
        show(FakeBackend(garbled = true))
        compose.onNodeWithText(text(R.string.calendar_unreachable)).assertIsDisplayed()
    }

    @Test
    fun aDayThatCannotBeSavedSaysTheServerIsOutOfReach() {
        val backend = FakeBackend()
        show(backend)
        cell(day(5), R.string.calendar_remote).performClick()
        backend.failing = true
        choice(R.string.calendar_office).performClick()
        compose.onNodeWithText(text(R.string.calendar_unreachable)).assertIsDisplayed()
        assertEquals(0, saves.get())
    }

    @Test
    fun aBuildWithoutServerShowsNoCalendar() {
        val backend = FakeBackend(configured = false)
        show(backend)
        compose.waitForIdle()
        compose.onNodeWithText(monthTitle(november)).assertDoesNotExist()
        assertTrue(backend.monthReads.isEmpty())
    }

    @Test
    fun theMonthIsReadAgainWhenAsked() {
        val backend = FakeBackend()
        val reads = AtomicInteger()
        var refresh by mutableIntStateOf(0)
        compose.setContent { CalendarPage(backend, today = today, refresh = refresh, onRead = { reads.incrementAndGet() }) }
        compose.waitUntil(timeoutMillis = 5_000) { reads.get() == 1 }
        refresh++
        compose.waitUntil(timeoutMillis = 5_000) { reads.get() == 2 }
        assertEquals(listOf(november, november), backend.monthReads.toList())
        // A day changed elsewhere, as the time away does, shows once read again.
        backend.set[day(5)] = CalendarDay(day(5), Kind.AWAY, Why.SET)
        refresh++
        waitForCell(day(5), R.string.calendar_away, set = true)
        compose.onNode(hasContentDescription(text(R.string.calendar_previous))).assertIsNotEnabled()
    }
}
