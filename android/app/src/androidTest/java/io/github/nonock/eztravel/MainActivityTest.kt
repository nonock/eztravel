package io.github.nonock.eztravel

import android.Manifest
import android.app.NotificationManager
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.core.view.WindowCompat
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.rule.GrantPermissionRule
import io.github.nonock.eztravel.backend.AppBackend
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.notifications.Notifier
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

        override fun before() {
            AppBackend.current = backend
        }

        override fun after() {
            AppBackend.current = original
        }
    }

    @get:Rule(order = 2)
    val compose = createAndroidComposeRule<MainActivity>()

    private fun text(id: Int) = compose.activity.getString(id)

    @Test
    fun theHomeScreenShowsTheStatusAndTheTimeAway() {
        compose.waitUntil(timeoutMillis = 5_000) { backend.statusReads.get() > 0 }
        compose.onNodeWithText(text(R.string.status_title)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.status_no_next)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.away_none)).assertIsDisplayed()
    }

    @Test
    fun savingTheTimeAwayReadsTheStatusAgain() {
        // Counted from the reads at launch, however many: the screen may resume more than once.
        compose.waitUntil(timeoutMillis = 5_000) { backend.statusReads.get() > 0 }
        compose.waitForIdle()
        val atLaunch = backend.statusReads.get()
        compose.onNodeWithText(text(R.string.away_choose)).performClick()
        compose.onNodeWithText(text(R.string.ok)).performClick()
        compose.waitUntil(timeoutMillis = 5_000) { backend.statusReads.get() > atLaunch }
    }

    @Test
    fun theHistoryOpensFromTheHomeScreenAndBackLeavesIt() {
        compose.onNodeWithText(text(R.string.history_title)).performClick()
        compose.onNodeWithText(text(R.string.history_empty)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.history_back)).performClick()
        compose.onNodeWithText(text(R.string.status_title)).assertIsDisplayed()
        // The phone's own back gesture does the same.
        compose.onNodeWithText(text(R.string.history_title)).performClick()
        compose.onNodeWithText(text(R.string.history_empty)).assertIsDisplayed()
        compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }
        compose.onNodeWithText(text(R.string.status_title)).assertIsDisplayed()
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
    }
}
