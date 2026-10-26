package io.github.nonock.eztravel

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nonock.eztravel.backend.Alerts
import io.github.nonock.eztravel.backend.Alerts.Alert
import io.github.nonock.eztravel.backend.Alerts.Cause
import io.github.nonock.eztravel.backend.Alerts.LineAlerts
import io.github.nonock.eztravel.backend.Alerts.Severity
import io.github.nonock.eztravel.backend.AppBackend
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.notifications.Opening
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import org.junit.After
import org.junit.Test
import org.junit.runner.RunWith
import java.time.Duration
import java.time.Instant

/** The app opened from a line's notification. */
@RunWith(AndroidJUnit4::class)
class OpeningTest : ScreenTesting() {
    private val original = AppBackend.current

    @After
    fun putBack() {
        AppBackend.current = original
    }

    private val now = Instant.now()

    private val stopped = Alert(
        id = "incident-l",
        severity = Severity.BLOCKING,
        cause = Cause.INCIDENT,
        title = "Ligne L : trafic interrompu",
        text = "Le trafic est interrompu entre Les Aubépines et Gare Centrale.",
        active = true,
        begin = now.minus(Duration.ofHours(1)),
        end = now.plus(Duration.ofHours(1)),
        onWay = true,
        updated = now,
    )

    private fun tapLabel(label: Int) = SemanticsMatcher("tapped to ${text(label)}") {
        it.config.getOrNull(SemanticsActions.OnClick)?.label == text(label)
    }

    private fun tab(label: Int) = hasText(text(label)) and hasClickAction()

    @Test
    fun aLineNotificationOpensTheAlertsTabOnItsRecordOnce() {
        AppBackend.current = FakeBackend(alerts = Alerts(listOf(LineAlerts(Line("L", Mode.TRAIN), null, listOf(stopped)))))
        compose.setContent { App(opening = Opening("L", "incident-l", null)) }
        waitFor(hasText(stopped.title) and tapLabel(R.string.alerts_fold))
        // Back on the commute, then on the alerts again: as the page would be.
        compose.onNode(tab(R.string.nav_trip)).performClick()
        compose.onNode(tab(R.string.nav_alerts)).performClick()
        waitFor(hasContentDescription(text(R.string.line_train, "L")) and tapLabel(R.string.alerts_show_line))
    }
}
