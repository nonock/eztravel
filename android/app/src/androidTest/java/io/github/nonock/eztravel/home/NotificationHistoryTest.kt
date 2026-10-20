package io.github.nonock.eztravel.home

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.backend.HistoryEntry
import io.github.nonock.eztravel.notifications.PushMessage
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import io.github.nonock.eztravel.notifications.text
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

@RunWith(AndroidJUnit4::class)
class NotificationHistoryTest {
    @get:Rule
    val compose = createComposeRule()

    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    private fun text(id: Int, vararg args: Any) = context.getString(id, *args)

    private fun show(backend: FakeBackend) = compose.setContent { NotificationHistory(backend) }

    /** Noon on the day `daysAgo` days before today, in the phone's time zone. */
    private fun noon(daysAgo: Long): Instant =
        LocalDate.now().minusDays(daysAgo).atTime(12, 0).atZone(ZoneId.systemDefault()).toInstant()

    private val train = Line("L", Mode.TRAIN)
    private val metro = Line("X", Mode.METRO)
    private val started = PushMessage.Started(train, Severity.BLOCKING, "Ligne L : trafic interrompu", "Panne d'un train.")
    private val ended = PushMessage.Ended(train)

    @Test
    fun entriesAreShownByDayNewestFirst() {
        val longAgo = noon(3)
        show(
            FakeBackend(
                history = listOf(
                    HistoryEntry.Sent(noon(0), ended),
                    HistoryEntry.Sent(noon(1), started),
                    HistoryEntry.WindowClosed(longAgo, metro, Severity.DISRUPTED),
                ),
            ),
        )
        compose.onNodeWithText(text(R.string.history_today)).assertIsDisplayed()
        compose.onNodeWithText(ended.text(context.resources).title).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.history_yesterday)).assertIsDisplayed()
        compose.onNodeWithText(started.text(context.resources).title).assertIsDisplayed()
        compose.onNodeWithText("Panne d'un train.").assertIsDisplayed()
        val day = longAgo.atZone(ZoneId.systemDefault()).toLocalDate()
        compose.onNodeWithText(DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL).format(day)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.history_closed_disrupted, text(R.string.line_metro, "X"))).assertIsDisplayed()
    }

    @Test
    fun aWindowClosingOnAnInterruptionSaysSo() {
        show(FakeBackend(history = listOf(HistoryEntry.WindowClosed(noon(0), train, Severity.BLOCKING))))
        compose.onNodeWithText(text(R.string.history_closed_blocking, text(R.string.line_train, "L"))).assertIsDisplayed()
    }

    @Test
    fun anEmptyHistorySaysSo() {
        show(FakeBackend())
        compose.onNodeWithText(text(R.string.history_empty)).assertIsDisplayed()
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
}
