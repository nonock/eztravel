package io.github.nonock.eztravel.home

import io.github.nonock.eztravel.ScreenTesting
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.platform.UriHandler
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Alerts
import io.github.nonock.eztravel.backend.Alerts.Alert
import io.github.nonock.eztravel.backend.Alerts.Cause
import io.github.nonock.eztravel.backend.Alerts.LineAlerts
import io.github.nonock.eztravel.backend.Alerts.Link
import io.github.nonock.eztravel.backend.Alerts.Severity
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.notifications.Opening
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import java.time.Duration
import java.time.Instant
import java.time.ZoneId
import java.util.concurrent.CopyOnWriteArrayList

@RunWith(AndroidJUnit4::class)
class AlertsPageTest : ScreenTesting() {
    private val now = Instant.now()
    private val today = now.atZone(ZoneId.systemDefault()).toLocalDate()

    private val train = Line("L", Mode.TRAIN)
    private val metro = Line("X", Mode.METRO)

    private fun alert(id: String, severity: Severity, title: String, active: Boolean, onWay: Boolean = false) = Alert(
        id = id,
        severity = severity,
        cause = if (active) Cause.INCIDENT else Cause.WORKS,
        title = title,
        text = "Le trafic est interrompu entre Les Aubépines et Gare Centrale.",
        active = active,
        begin = if (active) now.minus(Duration.ofHours(1)) else now.plus(Duration.ofDays(4)),
        end = if (active) now.plus(Duration.ofHours(1)) else now.plus(Duration.ofDays(4).plusHours(6)),
        onWay = onWay,
        updated = now,
    )

    private val stopped = alert("incident-l", Severity.BLOCKING, "Ligne L : trafic interrompu", active = true, onWay = true)
    private val works = alert("works-l", Severity.INFORMATION, "Ligne L : travaux le week-end", active = false)
    private val slowed = alert("slow-x", Severity.DISRUPTED, "Métro X : trafic ralenti", active = true)

    private fun show(backend: FakeBackend) = compose.setContent { AlertsPage(backend, now = { now }) }

    private val both = Alerts(
        listOf(
            LineAlerts(train, null, listOf(stopped, works)),
            LineAlerts(metro, null, listOf(slowed)),
        ),
    )

    @Test
    fun aNotificationOpensItsLineAndUnfoldsItsRecord() {
        compose.setContent { AlertsPage(FakeBackend(alerts = both), now = { now }, opening = Opening("L", "incident-l", null)) }
        waitFor(hasText(stopped.title) and tapLabel(R.string.alerts_fold))
        // The line's other records stay folded, and the other lines too.
        waitFor(hasText(works.title) and tapLabel(R.string.alerts_unfold))
        compose.onNodeWithText(slowed.title).assertDoesNotExist()
    }

    @Test
    fun aRecordGoneSinceStillOpensItsLine() {
        compose.setContent { AlertsPage(FakeBackend(alerts = both), now = { now }, opening = Opening("L", "gone", "An older title")) }
        waitFor(header(text(R.string.line_train, "L")) and tapLabel(R.string.alerts_hide_line))
        waitFor(hasText(stopped.title) and tapLabel(R.string.alerts_unfold))
    }

    private fun tapLabel(label: Int) = SemanticsMatcher("tapped to ${text(label)}") {
        it.config.getOrNull(SemanticsActions.OnClick)?.label == text(label)
    }

    /** A line's header, tapped to show or hide its alerts. */
    private fun header(line: String) = hasContentDescription(line) and hasClickAction()

    private fun unfold(line: String) {
        waitFor(header(line) and tapLabel(R.string.alerts_show_line))
        compose.onNode(header(line)).performClick()
    }

    /** A line's summary: how it runs, in words, when trouble is under way, then how many alerts. */
    private fun summary(underWay: Int, toCome: Int, state: Int? = null) = listOfNotNull(
        state?.let { text(it) },
        context.resources.getQuantityString(R.plurals.alerts_under_way, underWay, underWay),
        context.resources.getQuantityString(R.plurals.alerts_to_come, toCome, toCome).takeIf { toCome > 0 },
    ).joinToString(", ")

    @Test
    fun linesStartFoldedWithWhatIsUnderWayAndToCome() {
        val alerts = Alerts(
            listOf(
                LineAlerts(train, 0xFF7B61C9.toInt(), listOf(stopped, works, works.copy(id = "works-l-2"))),
                LineAlerts(metro, null, listOf(slowed)),
            ),
        )
        show(FakeBackend(alerts = alerts))
        val l = text(R.string.line_train, "L")
        // How bad, in words: the icon alone tells it by its color only.
        waitFor(header(l) and hasText(summary(1, 1, R.string.status_state_blocking)) and tapLabel(R.string.alerts_show_line))
        waitFor(header(text(R.string.line_metro, "X")) and hasText(summary(1, 0, R.string.status_state_disrupted)))
        compose.onNodeWithText(stopped.title).assertDoesNotExist()
        compose.onNode(header(l)).performClick()
        waitFor(hasText(stopped.title))
        waitFor(header(l) and tapLabel(R.string.alerts_hide_line))
        compose.onNode(header(l)).performClick()
        compose.waitUntil(timeoutMillis = 5_000) { compose.onAllNodes(hasText(stopped.title)).fetchSemanticsNodes().isEmpty() }
        // The metro's stays folded.
        compose.onNodeWithText(slowed.title).assertDoesNotExist()
    }

    @Test
    fun eachLineShowsItsAlertsWithWhenAndWhetherOnTheWay() {
        // Works come split by period: the second of a title shows as one more.
        val alerts = Alerts(
            listOf(
                LineAlerts(train, 0xFF7B61C9.toInt(), listOf(stopped, works, works.copy(id = "works-l-2"))),
                LineAlerts(metro, null, listOf(slowed)),
            ),
        )
        show(FakeBackend(alerts = alerts))
        waitFor(hasContentDescription(text(R.string.line_train, "L")))
        compose.onNodeWithText(text(R.string.line_train, "L")).assertDoesNotExist()
        unfold(text(R.string.line_train, "L"))
        unfold(text(R.string.line_metro, "X"))
        waitFor(
            hasText(stopped.title) and hasText(text(R.string.alerts_now)) and hasText(text(R.string.alerts_on_way)) and
                hasText(text(R.string.status_state_blocking)) and
                hasText(text(R.string.alerts_until, stopped.end.shown(today))) and hasText(stopped.text),
        )
        waitFor(hasText(slowed.title) and hasText(text(R.string.status_state_disrupted)))
        // Information is told by its own icon.
        compose.onNode(hasText(works.title) and hasText(text(R.string.status_state_disrupted))).assertDoesNotExist()
        compose.onNode(hasText(works.title) and hasText(text(R.string.status_state_blocking))).assertDoesNotExist()
        waitFor(
            hasText(works.title) and hasText(text(R.string.alerts_coming)) and
                hasText(text(R.string.alerts_from, works.begin.shown(today))) and
                hasText(context.resources.getQuantityString(R.plurals.alerts_more_periods, 1, 1)),
        )
        compose.onAllNodes(hasText(works.title)).fetchSemanticsNodes().let { assert(it.size == 1) { "${it.size} cards" } }
        waitFor(hasText(slowed.title) and hasText(text(R.string.alerts_now)))
        compose.onNodeWithText(text(R.string.alerts_on_way)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.alerts_none_at_all)).assertDoesNotExist()
    }

    @Test
    fun aCardUnfoldsWhenTappedAndFoldsAgain() {
        show(FakeBackend(alerts = Alerts(listOf(LineAlerts(train, null, listOf(stopped))))))
        unfold(text(R.string.line_train, "L"))
        waitFor(hasText(stopped.title) and tapLabel(R.string.alerts_unfold))
        compose.onNode(hasText(stopped.title)).performClick()
        waitFor(hasText(stopped.title) and tapLabel(R.string.alerts_fold))
        compose.onNode(hasText(stopped.title)).performClick()
        waitFor(hasText(stopped.title) and tapLabel(R.string.alerts_unfold))
    }

    @Test
    fun aRecordsLinksOpenOnceItIsUnfolded() {
        val url = "https://example.org/travaux-l"
        val linked = stopped.copy(text = "Détails sur le site de la ligne.", links = listOf(Link(15, 31, url)))
        val opened = CopyOnWriteArrayList<String>()
        // The browser is not opened in the test: the address is kept.
        val browser = object : UriHandler {
            override fun openUri(uri: String) {
                opened += uri
            }
        }
        compose.setContent {
            CompositionLocalProvider(LocalUriHandler provides browser) {
                AlertsPage(FakeBackend(alerts = Alerts(listOf(LineAlerts(train, null, listOf(linked))))), now = { now })
            }
        }
        unfold(text(R.string.line_train, "L"))
        waitFor(hasText(linked.title) and tapLabel(R.string.alerts_unfold))
        // Folded, the record holds no link: a tap only unfolds it.
        val link = SemanticsMatcher.keyIsDefined(SemanticsProperties.LinkTestMarker)
        compose.onAllNodes(link).assertCountEquals(0)
        compose.onNode(hasText(linked.title)).performClick()
        waitFor(hasText(linked.title) and tapLabel(R.string.alerts_fold))
        // In the tree screen readers see: a stop of its own.
        compose.onNode(link).performClick()
        compose.waitUntil { opened.isNotEmpty() }
        assertEquals(listOf(url), opened)
        // The record stays unfolded.
        compose.onNode(hasText(linked.title) and tapLabel(R.string.alerts_fold)).assertExists()
    }

    @Test
    fun noAlertAnywhereIsSaidSo() {
        show(FakeBackend(alerts = Alerts(listOf(LineAlerts(train, null, emptyList()), LineAlerts(metro, null, listOf(works))))))
        // A line without alerts has nothing to show; works to come only are said so.
        waitFor(hasContentDescription(text(R.string.line_train, "L")) and hasText(text(R.string.alerts_none)))
        compose.onNode(header(text(R.string.line_train, "L"))).assertDoesNotExist()
        waitFor(header(text(R.string.line_metro, "X")) and hasText(context.resources.getQuantityString(R.plurals.alerts_to_come, 1, 1)))
    }

    @Test
    fun noAlertOnAnyLineIsSaidOnTop() {
        show(FakeBackend(alerts = Alerts(listOf(LineAlerts(train, null, emptyList())))))
        waitFor(text(R.string.alerts_none_at_all))
    }

    @Test
    fun alertsTheBackendCannotReadAreNotTakenAsNone() {
        show(FakeBackend(alerts = Alerts(null)))
        waitFor(text(R.string.alerts_unavailable))
        compose.onNodeWithText(text(R.string.alerts_none_at_all)).assertDoesNotExist()
    }

    @Test
    fun anUnreachableServerLeavesTheAlertsUnknown() {
        show(FakeBackend(failing = true))
        waitFor(text(R.string.alerts_unavailable))
    }

    @Test
    fun aGarbledAnswerLeavesTheAlertsUnknown() {
        show(FakeBackend(garbled = true))
        waitFor(text(R.string.alerts_unavailable))
    }
}
