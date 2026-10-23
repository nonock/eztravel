package io.github.nonock.eztravel.settings

import android.os.ParcelFileDescriptor
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextReplacement
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.backend.Refusal
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.backend.UsualTrains
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.time.LocalTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.concurrent.atomic.AtomicInteger

@RunWith(AndroidJUnit4::class)
class UsualTrainSettingTest {
    @get:Rule
    val compose = createComposeRule()

    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    private fun text(id: Int, vararg args: Any) = context.getString(id, *args)

    private fun clock(hour: Int, minute: Int) = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).format(LocalTime.of(hour, minute))

    private val saves = AtomicInteger()

    private fun shell(command: String): String =
        ParcelFileDescriptor.AutoCloseInputStream(InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand(command))
            .bufferedReader()
            .use { it.readText().trim() }

    /** The phone's own choice of 12 or 24 hours, put back after each test. */
    private var hours = "null"

    /** Hours are typed from 0 to 23, whatever the device's language: an emulator tells time in 12 hours. */
    @Before
    fun twentyFourHours() {
        hours = shell("settings get system time_12_24")
        shell("settings put system time_12_24 24")
    }

    @After
    fun putTheHoursBack() {
        shell(if (hours == "null") "settings delete system time_12_24" else "settings put system time_12_24 $hours")
    }

    private fun show(backend: FakeBackend) = compose.setContent { UsualTrainSetting(backend, onSaved = { saves.incrementAndGet() }) }

    private fun waitFor(matcher: SemanticsMatcher) {
        compose.waitUntil(timeoutMillis = 5_000) { compose.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty() }
        compose.onNode(matcher).assertIsDisplayed()
    }

    /** A way's row, tapped to choose its usual train. */
    private fun way(label: Int) = SemanticsMatcher("tapped to ${text(label)}") {
        it.config.getOrNull(SemanticsActions.OnClick)?.label == text(label)
    }

    /** Types [hour] and [minute] in the open picker, and confirms. */
    private fun type(hour: String, minute: String) {
        // Two fields: hours, then minutes.
        compose.waitUntil(timeoutMillis = 5_000) { compose.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().size == 2 }
        val fields = compose.onAllNodes(hasSetTextAction())
        fields[0].performTextReplacement(hour)
        fields[1].performTextReplacement(minute)
        compose.onNodeWithText(text(R.string.ok)).performClick()
    }

    @Test
    fun eachWayShowsItsUsualTrainOrNone() {
        show(FakeBackend(usual = UsualTrains(LocalTime.of(8, 7), null)))
        waitFor(way(R.string.usual_choose_outbound) and hasText(text(R.string.usual_outbound)) and hasText(clock(8, 7)))
        waitFor(way(R.string.usual_choose_inbound) and hasText(text(R.string.usual_inbound)) and hasText(text(R.string.usual_none)))
        compose.onNodeWithText(text(R.string.usual_explained)).assertIsDisplayed()
        // Only a usual train can be removed.
        compose.onNodeWithContentDescription(text(R.string.usual_clear_outbound)).assertIsDisplayed()
        compose.onNodeWithContentDescription(text(R.string.usual_clear_inbound)).assertDoesNotExist()
    }

    @Test
    fun aUsualTrainIsChosenByItsTimeThenRemoved() {
        val backend = FakeBackend()
        show(backend)
        compose.onNode(way(R.string.usual_choose_inbound)).performClick()
        type("18", "52")
        waitFor(way(R.string.usual_choose_inbound) and hasText(clock(18, 52)))
        compose.onNodeWithContentDescription(text(R.string.usual_clear_inbound)).performClick()
        waitFor(way(R.string.usual_choose_inbound) and hasText(text(R.string.usual_none)))
        assertEquals(listOf(Direction.INBOUND to LocalTime.of(18, 52), Direction.INBOUND to null), backend.usualSaved)
        assertEquals(2, saves.get())
    }

    @Test
    fun thePickerOpensOnTheUsualTrainAndCanBeLeft() {
        val backend = FakeBackend(usual = UsualTrains(LocalTime.of(8, 7), null))
        show(backend)
        compose.onNode(way(R.string.usual_choose_outbound)).performClick()
        waitFor(hasSetTextAction() and hasText("08"))
        compose.onNodeWithText(text(R.string.cancel)).performClick()
        compose.onNode(way(R.string.usual_choose_inbound)).performClick()
        // Without a usual train, the picker opens on the evening.
        waitFor(hasSetTextAction() and hasText("18"))
        compose.onNodeWithText(text(R.string.ok)).performClick()
        waitFor(way(R.string.usual_choose_inbound) and hasText(clock(18, 0)))
        assertEquals(listOf(Direction.INBOUND to LocalTime.of(18, 0)), backend.usualSaved)
    }

    @Test
    fun aTrainThatWouldNeverBeRemindedIsRefused() {
        val backend = FakeBackend(usualRefusal = Refusal.OUTSIDE_WINDOW)
        show(backend)
        compose.onNode(way(R.string.usual_choose_outbound)).performClick()
        type("10", "15")
        waitFor(hasText(text(R.string.departures_refused_outside_window)))
        waitFor(way(R.string.usual_choose_outbound) and hasText(text(R.string.usual_none)))
        assertEquals(0, saves.get())
        // Any other reason, from a newer backend.
        backend.usualRefusal = Refusal.UNKNOWN
        compose.onNode(way(R.string.usual_choose_outbound)).performClick()
        type("08", "07")
        waitFor(hasText(text(R.string.usual_refused)))
        compose.onNodeWithText(text(R.string.departures_refused_outside_window)).assertDoesNotExist()
    }

    @Test
    fun aServerOutOfReachIsSaidSo() {
        val backend = FakeBackend()
        show(backend)
        waitFor(way(R.string.usual_choose_outbound))
        backend.failing = true
        compose.onNode(way(R.string.usual_choose_outbound)).performClick()
        type("08", "07")
        waitFor(hasText(text(R.string.usual_unreachable)))
    }

    @Test
    fun usualTrainsThatCannotBeReadAreSaidSo() {
        show(FakeBackend(garbled = true))
        waitFor(hasText(text(R.string.usual_unreachable)))
        compose.onNode(way(R.string.usual_choose_outbound)).assertDoesNotExist()
    }

    @Test
    fun aBuildWithoutServerShowsNoUsualTrain() {
        show(FakeBackend(configured = false))
        compose.waitForIdle()
        compose.onNodeWithText(text(R.string.usual_explained)).assertDoesNotExist()
    }
}
