package io.github.nonock.eztravel.watchdog

import android.Manifest
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.service.notification.StatusBarNotification
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.GrantPermissionRule
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.AppBackend
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.backend.Status
import io.github.nonock.eztravel.backend.Status.Cause
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.backend.Status.Outage
import io.github.nonock.eztravel.home.clock
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.time.Duration
import java.time.Instant
import java.util.concurrent.CopyOnWriteArrayList

@RunWith(AndroidJUnit4::class)
class WatchdogRunTest {
    @get:Rule
    val permission: GrantPermissionRule = GrantPermissionRule.grant(Manifest.permission.POST_NOTIFICATIONS)

    private val context: Context = InstrumentationRegistry.getInstrumentation().targetContext
    private val manager = context.getSystemService(NotificationManager::class.java)

    private fun text(id: Int, vararg args: Any) = context.getString(id, *args)

    private val now = Instant.parse("2026-11-03T06:50:00Z")

    private val backend = AppBackend.current
    private val alarm = Watchdog.schedule
    private val scheduled = CopyOnWriteArrayList<Instant?>()

    @Before
    fun fakes() {
        Watchdog.schedule = { _, at -> scheduled += at }
        Watchdog.forget(context)
        manager.cancelAll()
    }

    @After
    fun putBack() {
        AppBackend.current = backend
        Watchdog.schedule = alarm
        Watchdog.forget(context)
        manager.cancelAll()
    }

    private fun shown(): StatusBarNotification? = manager.activeNotifications.firstOrNull { it.tag == Watchdog.TAG }

    private fun StatusBarNotification.title() = notification.extras.getString(Notification.EXTRA_TITLE)

    private fun StatusBarNotification.body() = notification.extras.getString(Notification.EXTRA_TEXT)

    private fun watching(outage: Outage?) = Status(Direction.OUTBOUND, null, null, outage, emptyList(), emptyList())

    @Test
    fun aBackendThatDoesNotAnswerIsSaidSoAndTriedAgainForAWhile() = runBlocking {
        AppBackend.current = FakeBackend(failing = true)
        Watchdog.run(context, notify = true, now = now)
        assertEquals(text(R.string.watchdog_unreachable), shown()!!.title())
        assertEquals(text(R.string.watchdog_unreachable_text), shown()!!.body())
        assertEquals(listOf<Instant?>(now.plus(Duration.ofMinutes(20))), scheduled)
        // An answer that cannot be read is no answer: six in a row, then no more until the app is opened.
        AppBackend.current = FakeBackend(garbled = true)
        repeat(Watchdog.RETRIES - 1) { Watchdog.run(context, notify = true, now = now) }
        assertNull(scheduled.last())
    }

    @Test
    fun whatTheBackendCannotSayItselfIsSaid() = runBlocking {
        val since = Instant.parse("2026-11-03T06:30:00Z")
        for ((cause, title) in listOf(Cause.SERVER to R.string.watchdog_stopped, Cause.FIREBASE to R.string.watchdog_cannot_send)) {
            AppBackend.current = FakeBackend(status = watching(Outage(since, cause)))
            Watchdog.run(context, notify = true, now = now)
            assertEquals(text(title), shown()!!.title())
            assertEquals(text(R.string.watchdog_since, since.clock()), shown()!!.body())
        }
    }

    @Test
    fun onceTheBackendIsBackTheWarningGoesAndAnswersAreCountedAfresh() = runBlocking {
        AppBackend.current = FakeBackend(failing = true)
        Watchdog.run(context, notify = true, now = now)
        assertNotNull(shown())
        AppBackend.current = FakeBackend(status = watching(null))
        Watchdog.run(context, notify = false, now = now)
        assertNull(shown())
        AppBackend.current = FakeBackend(failing = true)
        repeat(Watchdog.RETRIES - 1) { Watchdog.run(context, notify = false, now = now) }
        assertNotNull(scheduled.last())
    }

    @Test
    fun whenTheAppOpensNothingIsSaidButTheNextLookIsSet() = runBlocking {
        AppBackend.current = FakeBackend(failing = true)
        Watchdog.run(context, notify = false, now = now)
        assertNull(shown())
        assertEquals(listOf<Instant?>(now.plus(Duration.ofMinutes(20))), scheduled)
    }

    @Test
    fun aBuildWithoutServerLooksAtNothing() = runBlocking {
        AppBackend.current = FakeBackend(configured = false, failing = true)
        Watchdog.run(context, notify = true, now = now)
        assertNull(shown())
        assertEquals(emptyList<Instant?>(), scheduled)
    }

    @Test
    fun theAlarmBringsTheLook() {
        AppBackend.current = FakeBackend(failing = true)
        context.sendBroadcast(Intent(context, WatchdogReceiver::class.java).setAction(WatchdogReceiver.ACTION_LOOK))
        val until = System.currentTimeMillis() + 10_000
        while (shown() == null && System.currentTimeMillis() < until) Thread.sleep(100)
        assertNotNull(shown())
    }

    @Test
    fun theNextLookIsAnAlarmThatCanBeCancelled() {
        fun pending() = PendingIntent.getBroadcast(
            context,
            0,
            Intent(context, WatchdogReceiver::class.java).setAction(WatchdogReceiver.ACTION_LOOK),
            PendingIntent.FLAG_NO_CREATE or PendingIntent.FLAG_IMMUTABLE,
        )
        alarm(context, Instant.now().plus(Duration.ofHours(1)))
        assertNotNull(pending())
        alarm(context, null)
        assertNull(pending())
    }
}
