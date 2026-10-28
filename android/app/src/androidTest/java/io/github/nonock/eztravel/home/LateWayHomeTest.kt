package io.github.nonock.eztravel.home

import android.os.ParcelFileDescriptor
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextReplacement
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.ScreenTesting
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.backend.Refusal
import io.github.nonock.eztravel.backend.Status
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.time.LocalTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.concurrent.atomic.AtomicInteger

@RunWith(AndroidJUnit4::class)
class LateWayHomeTest : ScreenTesting() {
    private fun clock(hour: Int, minute: Int) = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).format(LocalTime.of(hour, minute))

    private val saves = AtomicInteger()

    private fun shell(command: String): String =
        ParcelFileDescriptor.AutoCloseInputStream(InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand(command))
            .bufferedReader()
            .use { it.readText().trim() }

    /** The phone's own choice of 12 or 24 hours, put back after each test. */
    private var hours = "null"

    /** Hours are typed from 0 to 23, whatever the device's language. */
    @Before
    fun twentyFourHours() {
        hours = shell("settings get system time_12_24")
        shell("settings put system time_12_24 24")
    }

    @After
    fun putTheHoursBack() {
        shell(if (hours == "null") "settings delete system time_12_24" else "settings put system time_12_24 $hours")
    }

    private fun show(backend: FakeBackend, late: Status.Late? = null) = compose.setContent {
        LateWayHome(backend, late, now = { LocalTime.of(19, 12) }, onSaved = { saves.incrementAndGet() })
    }

    /** The two fields of the open picker, hours then minutes, as they show. */
    private fun shown(): Pair<String, String> {
        compose.waitUntil(timeoutMillis = 5_000) { compose.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().size == 2 }
        val fields = compose.onAllNodes(hasSetTextAction()).fetchSemanticsNodes()
        val text = { index: Int -> fields[index].config[androidx.compose.ui.semantics.SemanticsProperties.EditableText].text }
        return text(0) to text(1)
    }

    /** Types [hour] and [minute] in the open picker, and confirms. */
    private fun type(hour: String, minute: String) {
        shown()
        val fields = compose.onAllNodes(hasSetTextAction())
        fields[0].performTextReplacement(hour)
        fields[1].performTextReplacement(minute)
        compose.onNodeWithText(text(R.string.ok)).performClick()
    }

    @Test
    fun theWayHomeIsSetLaterAtTheTimeChosen() {
        val backend = FakeBackend()
        show(backend)
        compose.onNodeWithText(text(R.string.late_button)).performClick()
        // From the next full hour after now.
        assertEquals("20" to "00", shown())
        type("22", "30")
        compose.waitUntil(timeoutMillis = 5_000) { saves.get() == 1 }
        assertEquals(listOf(LocalTime.of(22, 30)), backend.lateSaved.toList())
    }

    @Test
    fun aLateWayHomeSaysUntilWhenItIsWatchedAndIsChangedOrPutBackAsUsual() {
        val backend = FakeBackend()
        show(backend, Status.Late(LocalTime.of(22, 0), LocalTime.of(23, 0), set = true))
        waitFor(text(R.string.late_set, clock(22, 0), clock(23, 0)))
        compose.onNode(hasText(text(R.string.late_button))).assertDoesNotExist()
        compose.onNodeWithText(text(R.string.late_change)).performClick()
        assertEquals("22" to "00", shown())
        type("23", "15")
        compose.waitUntil(timeoutMillis = 5_000) { saves.get() == 1 }
        compose.onNodeWithText(text(R.string.late_as_usual)).performClick()
        compose.waitUntil(timeoutMillis = 5_000) { saves.get() == 2 }
        assertEquals(listOf(LocalTime.of(23, 15), null), backend.lateSaved.toList())
    }

    @Test
    fun aDismissedPickerSavesNothing() {
        val backend = FakeBackend()
        show(backend)
        compose.onNodeWithText(text(R.string.late_button)).performClick()
        shown()
        compose.onNodeWithText(text(R.string.cancel)).performClick()
        compose.onNode(hasSetTextAction()).assertDoesNotExist()
        assertEquals(emptyList<LocalTime?>(), backend.lateSaved.toList())
    }

    @Test
    fun aRefusedWayHomeSaysWhy() {
        val backend = FakeBackend(lateRefusal = Refusal.NOT_OFFICE_DAY)
        show(backend)
        compose.onNodeWithText(text(R.string.late_button)).performClick()
        type("22", "00")
        waitFor(text(R.string.late_refused_not_office_day))
        backend.lateRefusal = Refusal.OUTSIDE_WINDOW
        compose.onNodeWithText(text(R.string.late_button)).performClick()
        type("15", "00")
        waitFor(text(R.string.late_refused_outside_window))
        assertEquals(0, saves.get())
    }

    @Test
    fun aWayHomeThatCannotBeSavedSaysSo() {
        show(FakeBackend(failing = true), Status.Late(LocalTime.of(20, 0), LocalTime.of(21, 0), set = false))
        compose.onNodeWithText(text(R.string.late_as_usual)).performClick()
        waitFor(text(R.string.late_failed))
        assertEquals(0, saves.get())
    }
}
