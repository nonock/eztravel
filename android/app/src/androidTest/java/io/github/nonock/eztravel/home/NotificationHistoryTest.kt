package io.github.nonock.eztravel.home

import io.github.nonock.eztravel.ScreenTesting
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onNodeWithText
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.backend.HistoryEntry
import io.github.nonock.eztravel.notifications.PushMessage
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import io.github.nonock.eztravel.notifications.text
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import org.junit.Test
import org.junit.runner.RunWith
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

@RunWith(AndroidJUnit4::class)
class NotificationHistoryTest : ScreenTesting() {
    private fun show(backend: FakeBackend) = compose.setContent { NotificationHistory(backend) }

    /** Noon on the day `daysAgo` days before today, in the phone's time zone. */
    private fun noon(daysAgo: Long): Instant =
        LocalDate.now().minusDays(daysAgo).atTime(12, 0).atZone(ZoneId.systemDefault()).toInstant()

    private val train = Line("L", Mode.TRAIN)
    private val metro = Line("X", Mode.METRO)
    private val started = PushMessage.Started(
        train,
        Severity.BLOCKING,
        "Ligne L : trafic interrompu",
        "Panne d'un train.",
        scope = PushMessage.Scope.Section("Les Aubépines", "Gare Centrale"),
    )
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
        // Where, in one line, as the notification said it.
        compose.onNodeWithText(text(R.string.notification_where_section, "Les Aubépines", "Gare Centrale")).assertIsDisplayed()
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
        compose.onNodeWithText(text(R.string.unreachable)).assertIsDisplayed()
    }

    @Test
    fun eachEntryShowsWhatItIsAbout() {
        val started = PushMessage.Started(train, PushMessage.Severity.BLOCKING, "Ligne L : trafic interrompu", null)
        val train1 = PushMessage.Train(train, "PALO", "Gare Centrale", "Les Aubépines", noon(0), noon(0))
        show(
            FakeBackend(
                history = listOf(
                    HistoryEntry.Sent(noon(0), started),
                    HistoryEntry.Sent(noon(0), PushMessage.Leave(train1, noon(0), "2", live = true, until = null)),
                    HistoryEntry.Sent(noon(0), PushMessage.Outage),
                    HistoryEntry.Sent(noon(0), PushMessage.Recovered),
                    HistoryEntry.Sent(noon(0), PushMessage.Planned(1, "Ligne L : travaux")),
                ),
            ),
        )
        compose.waitUntil(timeoutMillis = 5_000) {
            compose.onAllNodes(hasText("Ligne L : trafic interrompu")).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNode(hasContentDescription(text(R.string.line_train, "L")) and hasText("Ligne L : trafic interrompu")).assertIsDisplayed()
    }

    @Test
    fun aGarbledAnswerIsTakenAsAnUnreachableServer() {
        show(FakeBackend(garbled = true))
        compose.onNodeWithText(text(R.string.unreachable)).assertIsDisplayed()
    }
}
