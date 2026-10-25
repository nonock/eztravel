package io.github.nonock.eztravel

import android.Manifest
import android.app.NotificationManager
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeDown
import androidx.core.view.WindowCompat
import androidx.test.espresso.Espresso
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.rule.GrantPermissionRule
import io.github.nonock.eztravel.backend.AppBackend
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.notifications.Notifier
import io.github.nonock.eztravel.calendar.monthTitle
import io.github.nonock.eztravel.notifications.PushToken
import io.github.nonock.eztravel.watchdog.Watchdog
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Rule
import org.junit.Test
import org.junit.rules.ExternalResource
import org.junit.runner.RunWith
import java.time.Instant
import java.time.LocalDate
import java.time.YearMonth
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.concurrent.CopyOnWriteArrayList

@RunWith(AndroidJUnit4::class)
class MainActivityTest {
    private val backend = FakeBackend()

    /** The watchdog's next looks, as set: from the launch on. */
    private val looks = CopyOnWriteArrayList<Instant?>()

    @get:Rule(order = 0)
    val permission: GrantPermissionRule = GrantPermissionRule.grant(Manifest.permission.POST_NOTIFICATIONS)

    /** In place before the activity starts, so that no test reaches the real backend. */
    @get:Rule(order = 1)
    val fakeBackend = object : ExternalResource() {
        private val original = AppBackend.current
        private val token = PushToken.fetch
        private val alarm = Watchdog.schedule

        override fun before() {
            AppBackend.current = backend
            // Firebase gives no token without Google's services, as on an emulator.
            PushToken.fetch = { onToken -> onToken("token-from-firebase") }
            Watchdog.schedule = { _, at -> looks += at }
        }

        override fun after() {
            AppBackend.current = original
            PushToken.fetch = token
            Watchdog.schedule = alarm
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
        compose.onNodeWithText(text(R.string.departures_title)).assertIsDisplayed()
        // How the watch goes, in an icon beside the gear: nothing is watched now.
        compose.onNodeWithContentDescription(text(R.string.status_not_watching)).assertIsDisplayed()
    }

    @Test
    fun theStatusIconOpensAllTheBackendSees() {
        compose.waitUntil(timeoutMillis = 5_000) { backend.statusReads.get() > 0 }
        compose.onNodeWithContentDescription(text(R.string.status_not_watching)).performClick()
        compose.onNodeWithText(text(R.string.status_no_next)).assertIsDisplayed()
        // Back closes it, on the commute still: the sheet is a window of its own.
        Espresso.pressBack()
        compose.onNodeWithText(text(R.string.status_no_next)).assertDoesNotExist()
        tab(R.string.nav_trip).assertIsSelected()
        // From the alerts too.
        tab(R.string.nav_alerts).performClick()
        compose.onNodeWithContentDescription(text(R.string.status_not_watching)).performClick()
        compose.onNodeWithText(text(R.string.status_no_next)).assertIsDisplayed()
    }

    @Test
    fun eachTabOpensItsPageAndBackComesToTheCommute() {
        tab(R.string.nav_alerts).performClick()
        tab(R.string.nav_alerts).assertIsSelected()
        compose.onNodeWithText(text(R.string.alerts_none_at_all)).assertIsDisplayed()
        // The phone's own back gesture comes to the commute, as its tab does.
        back()
        compose.onNodeWithText(text(R.string.departures_title)).assertIsDisplayed()
        tab(R.string.nav_alerts).performClick()
        tab(R.string.nav_trip).performClick()
        compose.onNodeWithText(text(R.string.departures_title)).assertIsDisplayed()
        // The history and the calendar are behind the gear.
        tab(R.string.nav_history).assertDoesNotExist()
        tab(R.string.nav_calendar).assertDoesNotExist()
    }

    /** Opens a page behind the gear by its entry. */
    private fun open(label: Int) {
        compose.onNodeWithContentDescription(text(R.string.settings_title)).performClick()
        compose.onNode(hasText(text(label)) and hasClickAction()).performClick()
    }

    @Test
    fun theGearOpensTheCalendarTheUsualTrainsAndTheHistory() {
        compose.onNodeWithContentDescription(text(R.string.settings_title)).performClick()
        for (entry in listOf(R.string.nav_calendar, R.string.usual_title, R.string.nav_history)) {
            compose.onNode(hasText(text(entry)) and hasClickAction()).assertIsDisplayed()
        }
        tab(R.string.nav_trip).assertDoesNotExist()
        compose.onNode(hasText(text(R.string.nav_calendar)) and hasClickAction()).performClick()
        compose.onNodeWithText(monthTitle(YearMonth.now())).assertIsDisplayed()
        // Back comes to the list, then to the commute.
        compose.onNodeWithContentDescription(text(R.string.back)).performClick()
        compose.onNode(hasText(text(R.string.usual_title)) and hasClickAction()).performClick()
        compose.onNodeWithText(text(R.string.usual_explained)).assertIsDisplayed()
        back()
        compose.onNode(hasText(text(R.string.nav_history)) and hasClickAction()).performClick()
        compose.onNodeWithText(text(R.string.history_title)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.history_empty)).assertIsDisplayed()
        back()
        back()
        compose.onNodeWithText(text(R.string.departures_title)).assertIsDisplayed()
        compose.onNodeWithContentDescription(text(R.string.settings_title)).performClick()
        compose.onNodeWithContentDescription(text(R.string.back)).performClick()
        compose.onNodeWithText(text(R.string.departures_title)).assertIsDisplayed()
    }

    /** Pulls the page down, from its top, and waits for each count of [reads] to grow. */
    private fun pullDown(vararg reads: () -> Int) {
        compose.waitUntil(timeoutMillis = 5_000) { reads.all { it() > 0 } }
        compose.waitForIdle()
        val before = reads.map { it() }
        compose.onNode(hasScrollAction()).performTouchInput { swipeDown(startY = top, endY = bottom) }
        compose.waitUntil(timeoutMillis = 5_000) { reads.zip(before).all { (read, was) -> read() > was } }
    }

    @Test
    fun pullingEachPageDownReadsItAgain() {
        pullDown({ backend.statusReads.get() }, { backend.departuresReads.get() })
        tab(R.string.nav_alerts).performClick()
        pullDown({ backend.alertsReads.get() })
        open(R.string.nav_history)
        pullDown({ backend.historyReads.get() })
        // And again: the spinner went away after the last read.
        pullDown({ backend.historyReads.get() })
        back()
        compose.onNode(hasText(text(R.string.nav_calendar)) and hasClickAction()).performClick()
        pullDown({ backend.monthReads.size })
    }

    @Test
    fun savingAUsualTrainReadsTheDeparturesAgain() {
        compose.waitUntil(timeoutMillis = 5_000) { backend.departuresReads.get() > 0 }
        compose.waitForIdle()
        val atLaunch = backend.departuresReads.get()
        open(R.string.usual_title)
        compose.onNode(
            SemanticsMatcher("tapped to choose") {
                it.config.getOrNull(SemanticsActions.OnClick)?.label == text(R.string.usual_choose_outbound)
            },
        ).performClick()
        compose.onNodeWithText(text(R.string.ok)).performClick()
        back()
        back()
        compose.waitUntil(timeoutMillis = 5_000) { backend.departuresReads.get() > atLaunch }
    }

    @Test
    fun settingADayLooksAtTheBackendAgain() {
        // A day at the office may come before the next look: it is set again.
        open(R.string.nav_calendar)
        compose.waitUntil(timeoutMillis = 5_000) { backend.monthReads.isNotEmpty() && looks.isNotEmpty() }
        compose.waitForIdle()
        val looked = looks.size
        val today = DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL).format(LocalDate.now())
        compose.onNode(hasContentDescription(today, substring = true) and hasClickAction()).performClick()
        compose.onNode(hasText(text(R.string.calendar_away)) and hasClickAction()).performClick()
        compose.waitUntil(timeoutMillis = 5_000) { looks.size > looked }
        assertEquals(LocalDate.now(), backend.daysSet.single().first)
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
