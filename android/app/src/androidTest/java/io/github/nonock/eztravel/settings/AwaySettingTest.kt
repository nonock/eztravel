package io.github.nonock.eztravel.settings

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.FakeBackend
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

@RunWith(AndroidJUnit4::class)
class AwaySettingTest {
    @get:Rule
    val compose = createComposeRule()

    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    private fun text(id: Int, vararg args: Any) = context.getString(id, *args)

    private fun longDate(date: LocalDate) = DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL).format(date)

    private fun show(backend: FakeBackend) = compose.setContent { AwaySetting(backend) }

    @Test
    fun withoutTimeAwayItSaysSo() {
        show(FakeBackend())
        compose.onNodeWithText(text(R.string.away_none)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.away_clear)).assertDoesNotExist()
    }

    @Test
    fun theLastDayAwayIsShownAndCanBeCleared() {
        val backend = FakeBackend(away = LocalDate.of(2026, 12, 24))
        show(backend)
        compose.onNodeWithText(text(R.string.away_until, longDate(LocalDate.of(2026, 12, 24)))).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.away_clear)).performClick()
        compose.onNodeWithText(text(R.string.away_none)).assertIsDisplayed()
        assertEquals(listOf("null"), backend.saved)
    }

    @Test
    fun aDayIsChosenInTheDatePicker() {
        val backend = FakeBackend()
        show(backend)
        compose.onNodeWithText(text(R.string.away_choose)).performClick()
        // The picker opens on today: confirming keeps it.
        compose.onNodeWithText(text(R.string.ok)).performClick()
        val today = LocalDate.now()
        compose.onNodeWithText(text(R.string.away_until, longDate(today))).assertIsDisplayed()
        assertEquals(listOf(today.toString()), backend.saved)
    }

    @Test
    fun theDatePickerCanBeDismissed() {
        val backend = FakeBackend()
        show(backend)
        compose.onNodeWithText(text(R.string.away_choose)).performClick()
        compose.onNodeWithText(text(R.string.cancel)).performClick()
        compose.onNodeWithText(text(R.string.away_none)).assertIsDisplayed()
        assertEquals(emptyList<String>(), backend.saved)
    }

    @Test
    fun anUnreachableServerIsSaid() {
        show(FakeBackend(failing = true))
        compose.onNodeWithText(text(R.string.away_unreachable)).assertIsDisplayed()
    }

    @Test
    fun aGarbledAnswerIsTakenAsAnUnreachableServer() {
        show(FakeBackend(garbled = true))
        compose.onNodeWithText(text(R.string.away_unreachable)).assertIsDisplayed()
    }

    @Test
    fun aFailedSaveIsSaid() {
        val backend = FakeBackend(away = LocalDate.of(2026, 12, 24))
        show(backend)
        compose.onNodeWithText(text(R.string.away_clear)).assertIsDisplayed()
        backend.failing = true
        compose.onNodeWithText(text(R.string.away_clear)).performClick()
        compose.onNodeWithText(text(R.string.away_unreachable)).assertIsDisplayed()
    }

    @Test
    fun aBuildWithoutServerSaysItIsNotConfigured() {
        show(FakeBackend(configured = false))
        compose.onNodeWithText(text(R.string.away_not_configured)).assertIsDisplayed()
    }
}
