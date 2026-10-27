package io.github.nonock.eztravel.notifications

import android.Manifest
import android.app.Notification
import android.app.NotificationManager
import android.os.SystemClock
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.GrantPermissionRule
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.AppBackend
import io.github.nonock.eztravel.backend.CalendarDay.Kind
import io.github.nonock.eztravel.backend.DayRefusal
import io.github.nonock.eztravel.backend.FakeBackend
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.time.LocalDate

/** The evening notice's button: tomorrow worked from home, without opening the app. */
@RunWith(AndroidJUnit4::class)
class TomorrowActionTest {
    @get:Rule
    val permission: GrantPermissionRule = GrantPermissionRule.grant(Manifest.permission.POST_NOTIFICATIONS)

    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val manager = context.getSystemService(NotificationManager::class.java)
    private val backend = AppBackend.current
    private val tomorrow = LocalDate.now().plusDays(1)
    private val notice = PushMessage.Tomorrow(tomorrow, 1, "Ligne L : travaux", until = null)

    @After
    fun putBack() {
        AppBackend.current = backend
        manager.cancelAll()
    }

    private fun shown(): Notification? = manager.activeNotifications.singleOrNull { it.tag == "tomorrow" }?.notification

    /** Posting is asynchronous: wait for the system. */
    private fun waitFor(what: String, done: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 5_000
        while (!done() && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(50)
        assertTrue(what, done())
    }

    private fun waitForTitle(expected: String) = waitFor("titled \"$expected\"") {
        shown()?.extras?.getString(Notification.EXTRA_TITLE) == expected
    }

    private fun remoteButton(): Notification.Action? =
        shown()?.actions?.singleOrNull { it.title == context.getString(R.string.notification_remote_tomorrow) }

    @Test
    fun theButtonSetsTomorrowToRemoteWorkAndSaysSo() {
        val fake = FakeBackend()
        AppBackend.current = fake
        Notifier.show(context, notice)
        waitForTitle(notice.text(context.resources).title)
        remoteButton()!!.actionIntent.send()
        waitForTitle(context.getString(R.string.notification_tomorrow_remote))
        assertEquals(listOf(Triple(tomorrow, tomorrow, Kind.REMOTE)), fake.daysSet.toList())
        assertEquals(
            context.getString(R.string.notification_tomorrow_remote_text),
            shown()!!.extras.getCharSequence(Notification.EXTRA_TEXT).toString(),
        )
        // Done: nothing left to tap but the notice.
        assertEquals(null, remoteButton())
    }

    @Test
    fun whenTheDayCannotBeSetTheButtonStaysToTryAgain() {
        val fake = FakeBackend(failing = true)
        AppBackend.current = fake
        Notifier.show(context, notice)
        waitForTitle(notice.text(context.resources).title)
        remoteButton()!!.actionIntent.send()
        waitForTitle(context.getString(R.string.notification_tomorrow_unchanged))
        // An answer that cannot be read, from a server in a bad state.
        fake.failing = false
        fake.garbled = true
        Notifier.show(context, notice)
        waitForTitle(notice.text(context.resources).title)
        remoteButton()!!.actionIntent.send()
        waitForTitle(context.getString(R.string.notification_tomorrow_unchanged))
        // Refused, as when tapped too late: not changed either.
        fake.garbled = false
        fake.dayRefusal = DayRefusal.PAST
        remoteButton()!!.actionIntent.send()
        waitFor("set once") { fake.daysSet.size == 1 }
        waitForTitle(context.getString(R.string.notification_tomorrow_unchanged))
        fake.dayRefusal = null
        remoteButton()!!.actionIntent.send()
        waitForTitle(context.getString(R.string.notification_tomorrow_remote))
    }
}
