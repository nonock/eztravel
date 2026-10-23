package io.github.nonock.eztravel

import android.Manifest
import android.app.NotificationManager
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeDown
import androidx.core.view.WindowCompat
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.rule.GrantPermissionRule
import io.github.nonock.eztravel.backend.AppBackend
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.notifications.Notifier
import io.github.nonock.eztravel.notifications.PushToken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Rule
import org.junit.Test
import org.junit.rules.ExternalResource
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class MainActivityTest {
    private val backend = FakeBackend()

    @get:Rule(order = 0)
    val permission: GrantPermissionRule = GrantPermissionRule.grant(Manifest.permission.POST_NOTIFICATIONS)

    /** In place before the activity starts, so that no test reaches the real backend. */
    @get:Rule(order = 1)
    val fakeBackend = object : ExternalResource() {
        private val original = AppBackend.current
        private val token = PushToken.fetch

        override fun before() {
            AppBackend.current = backend
            // Firebase gives no token without Google's services, as on an emulator.
            PushToken.fetch = { onToken -> onToken("token-from-firebase") }
        }

        override fun after() {
            AppBackend.current = original
            PushToken.fetch = token
        }
    }

    @get:Rule(order = 2)
    val compose = createAndroidComposeRule<MainActivity>()

    private fun text(id: Int) = compose.activity.getString(id)

    /** A tab of the bottom bar. */
    private fun tab(id: Int) = compose.onNode(hasText(text(id)) and hasClickAction())

    /** The phone's back gesture, once the screen shows what was last tapped. */
    private fun back() {
        compose.waitForIdle()
        compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }
    }

    @Test
    fun theAppOpensOnTheCommute() {
        compose.waitUntil(timeoutMillis = 5_000) { backend.statusReads.get() > 0 }
        tab(R.string.nav_trip).assertIsSelected()
        compose.onNodeWithText(text(R.string.app_name)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.status_title)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.status_no_next)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.departures_title)).assertIsDisplayed()
    }

    @Test
    fun eachTabOpensItsPageAndBackComesToTheCommute() {
        tab(R.string.nav_history).performClick()
        compose.onNodeWithText(text(R.string.history_title)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.history_empty)).assertIsDisplayed()
        tab(R.string.nav_alerts).performClick()
        tab(R.string.nav_alerts).assertIsSelected()
        compose.onNodeWithText(text(R.string.alerts_none_at_all)).assertIsDisplayed()
        // The phone's own back gesture comes to the commute, as its tab does.
        back()
        compose.onNodeWithText(text(R.string.status_title)).assertIsDisplayed()
        tab(R.string.nav_history).performClick()
        tab(R.string.nav_trip).performClick()
        compose.onNodeWithText(text(R.string.status_title)).assertIsDisplayed()
    }

    @Test
    fun theTimeAwayIsBehindTheGearAndBackLeavesIt() {
        compose.onNodeWithContentDescription(text(R.string.settings_title)).performClick()
        compose.onNodeWithText(text(R.string.usual_title)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.away_none)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.away_title)).assertIsDisplayed()
        compose.onNodeWithContentDescription(text(R.string.back)).performClick()
        compose.onNodeWithText(text(R.string.status_title)).assertIsDisplayed()
        compose.onNodeWithContentDescription(text(R.string.settings_title)).performClick()
        back()
        compose.onNodeWithText(text(R.string.status_title)).assertIsDisplayed()
    }

    /** Pulls the page down, from its top, and waits for [reads] to grow. */
    private fun pullDown(vararg reads: java.util.concurrent.atomic.AtomicInteger) {
        compose.waitUntil(timeoutMillis = 5_000) { reads.all { it.get() > 0 } }
        compose.waitForIdle()
        val before = reads.map { it.get() }
        compose.onNode(hasScrollAction()).performTouchInput { swipeDown(startY = top, endY = bottom) }
        compose.waitUntil(timeoutMillis = 5_000) { reads.zip(before).all { (read, was) -> read.get() > was } }
    }

    @Test
    fun pullingEachTabDownReadsItAgain() {
        pullDown(backend.statusReads, backend.departuresReads)
        tab(R.string.nav_alerts).performClick()
        pullDown(backend.alertsReads)
        tab(R.string.nav_history).performClick()
        pullDown(backend.historyReads)
        // And again: the spinner went away after the last read.
        pullDown(backend.historyReads)
    }

    @Test
    fun savingAUsualTrainReadsTheDeparturesAgain() {
        compose.waitUntil(timeoutMillis = 5_000) { backend.departuresReads.get() > 0 }
        compose.waitForIdle()
        val atLaunch = backend.departuresReads.get()
        compose.onNodeWithContentDescription(text(R.string.settings_title)).performClick()
        compose.onNode(
            SemanticsMatcher("tapped to choose") {
                it.config.getOrNull(SemanticsActions.OnClick)?.label == text(R.string.usual_choose_outbound)
            },
        ).performClick()
        compose.onNodeWithText(text(R.string.ok)).performClick()
        compose.onNodeWithContentDescription(text(R.string.back)).performClick()
        compose.waitUntil(timeoutMillis = 5_000) { backend.departuresReads.get() > atLaunch }
    }

    @Test
    fun savingTheTimeAwayReadsTheStatusAgain() {
        // Counted from the reads at launch, however many: the screen may resume more than once.
        compose.waitUntil(timeoutMillis = 5_000) { backend.statusReads.get() > 0 }
        compose.waitForIdle()
        val atLaunch = backend.statusReads.get()
        compose.onNodeWithContentDescription(text(R.string.settings_title)).performClick()
        compose.onNodeWithText(text(R.string.away_choose)).performClick()
        compose.onNodeWithText(text(R.string.ok)).performClick()
        compose.onNodeWithContentDescription(text(R.string.back)).performClick()
        compose.waitUntil(timeoutMillis = 5_000) { backend.statusReads.get() > atLaunch }
    }

    @Test
    fun theStatusBarIconsAreLightOnTheNavyBackground() {
        compose.runOnUiThread {
            val window = compose.activity.window
            assertFalse(WindowCompat.getInsetsController(window, window.decorView).isAppearanceLightStatusBars)
        }
    }

    @Test
    fun theDisruptionsChannelExistsFromTheFirstLaunch() {
        // Notifications can come while the app is closed: the channel must already be there.
        val manager = compose.activity.getSystemService(NotificationManager::class.java)
        assertNotNull(manager.getNotificationChannel(Notifier.CHANNEL))
    }

    @Test
    fun thePhoneRegistersWithTheBackendAtLaunch() {
        // Firebase gives the token, then the app hands it to the backend.
        compose.waitUntil(timeoutMillis = 15_000) { backend.registered.isNotEmpty() }
        assertEquals(listOf("token-from-firebase"), backend.registered.distinct())
    }
}
