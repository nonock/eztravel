package io.github.nonock.eztravel.notifications

import android.Manifest
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.os.Build
import android.os.SystemClock
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.GrantPermissionRule
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.watchdog.Watchdog
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.time.Duration
import java.time.Instant
import java.time.temporal.ChronoUnit

@RunWith(AndroidJUnit4::class)
class LiveTripTest {
    @get:Rule
    val permission: GrantPermissionRule = GrantPermissionRule.grant(Manifest.permission.POST_NOTIFICATIONS)

    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val manager = context.getSystemService(NotificationManager::class.java)
    private val now = Instant.now().truncatedTo(ChronoUnit.SECONDS)
    private val train = PushMessage.Train(
        Line("L", Mode.TRAIN),
        "PALO",
        "Gare Centrale",
        "Les Aubépines",
        aimed = now.plus(Duration.ofMinutes(20)),
        expected = now.plus(Duration.ofMinutes(22)),
    )
    private val leave = PushMessage.Leave(train, now.plus(Duration.ofMinutes(10)), "2", live = true, until = train.expected!!.plus(Duration.ofMinutes(10)))

    @After
    fun clear() {
        LiveTrip.end(context)
        manager.cancelAll()
    }

    private fun trip(): Notification? = manager.activeNotifications.singleOrNull { it.tag == LiveTrip.TAG }?.notification

    private fun title(): String? = trip()?.extras?.getString(Notification.EXTRA_TITLE)

    /** Posting is asynchronous and limited to a few updates per second per app: wait for the system. */
    private fun waitFor(what: String, condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 3_000
        while (!condition() && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(50)
        assertTrue(what, condition())
    }

    private fun alarmSet(): Boolean = LiveTrip.alarm(context, null, PendingIntent.FLAG_NO_CREATE) != null

    @Test
    fun theReminderToLeaveIsOneOngoingTripCountingDownToLeaving() {
        Notifier.show(context, leave, now)
        waitFor("the trip shows") { title() == leave.text(context.resources).title }
        val trip = trip()!!
        assertEquals(Notifier.CHANNEL, trip.channelId)
        assertTrue(trip.flags and Notification.FLAG_ONGOING_EVENT != 0)
        assertEquals(leave.leave.toEpochMilli(), trip.`when`)
        assertTrue(trip.extras.getBoolean(Notification.EXTRA_SHOW_CHRONOMETER))
        assertTrue(trip.extras.getBoolean(Notification.EXTRA_CHRONOMETER_COUNT_DOWN))
        // A Live Update, where Android has them.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA) assertTrue(trip.hasPromotableCharacteristics())
        // Gone once the train has left.
        assertEquals(Duration.between(now, leave.until).toMillis(), trip.timeoutAfter)
        // In place of the train's own notification, with the time to leave set to go on the way.
        assertEquals(emptyList<Any>(), manager.activeNotifications.filter { it.tag == leave.tag })
        assertTrue(alarmSet())
    }

    @Test
    fun onceTimeToLeaveTheTripCountsDownToTheTrain() {
        Notifier.show(context, leave, now)
        waitFor("the trip shows") { title() == leave.text(context.resources).title }
        // The alarm at the time to leave.
        LiveTripReceiver().onReceive(context, LiveTrip.onTheWayIntent(context, leave))
        waitFor("on the way") { title() == leave.onTheWay(context.resources).title }
        assertEquals(train.expected!!.toEpochMilli(), trip()!!.`when`)
        assertEquals(leave.text(context.resources).body, trip()!!.extras.getCharSequence(Notification.EXTRA_BIG_TEXT).toString())
    }

    @Test
    fun aReminderComingPastTheTimeToLeaveCountsDownToTheTrainAtOnce() {
        Notifier.show(context, leave, leave.leave.plus(Duration.ofMinutes(1)))
        waitFor("on the way") { title() == leave.onTheWay(context.resources).title }
        assertEquals(train.expected!!.toEpochMilli(), trip()!!.`when`)
        assertFalse(alarmSet())
    }

    @Test
    fun aTripPastItsTrainGoes() {
        Notifier.show(context, leave, now)
        waitFor("the trip shows") { trip() != null }
        // The alarm, brought late by the phone, after the train has left.
        LiveTrip.onTheWay(context, LiveTrip.onTheWayIntent(context, leave), leave.until!!.plus(Duration.ofMinutes(1)))
        waitFor("the trip ends") { trip() == null }
        assertFalse(alarmSet())
    }

    @Test
    fun withoutExactAlarmsTheTripStillGoesOnTheWay() {
        val exact = Watchdog.exactAllowed
        Watchdog.exactAllowed = { false }
        try {
            Notifier.show(context, leave, now)
            waitFor("the trip shows") { trip() != null }
            assertTrue(alarmSet())
        } finally {
            Watchdog.exactAllowed = exact
        }
    }

    @Test
    fun anAlarmMissingWhatToShowShowsNothing() {
        Notifier.show(context, leave, now)
        waitFor("the trip shows") { trip() != null }
        LiveTrip.onTheWay(context, LiveTrip.onTheWayIntent(context, leave).apply { removeExtra("title") })
        SystemClock.sleep(500)
        assertEquals(leave.text(context.resources).title, title())
    }

    @Test
    fun aTripDismissedIsNotBroughtBackOnTheWay() {
        Notifier.show(context, leave, now)
        waitFor("the trip shows") { trip() != null }
        manager.cancel(LiveTrip.TAG, 1)
        waitFor("dismissed") { trip() == null }
        LiveTrip.onTheWay(context, LiveTrip.onTheWayIntent(context, leave))
        SystemClock.sleep(500)
        assertNull(trip())
    }

    @Test
    fun theTrainRunningLateUpdatesTheTrip() {
        Notifier.show(context, leave, now)
        waitFor("the trip shows") { trip() != null }
        val late = PushMessage.TrainLate(train.copy(expected = train.aimed.plus(Duration.ofMinutes(6))), leave.until)
        Notifier.show(context, late, now)
        waitFor("late") { title() == late.text(context.resources).title }
        assertEquals(late.train.expected!!.toEpochMilli(), trip()!!.`when`)
        assertTrue(trip()!!.flags and Notification.FLAG_ONGOING_EVENT != 0)
        // No longer brought back to the reminder's times.
        assertFalse(alarmSet())
        assertEquals(1, manager.activeNotifications.count { it.tag == LiveTrip.TAG })
    }

    @Test
    fun theTrainCancelledEndsTheTrip() {
        Notifier.show(context, leave, now)
        waitFor("the trip shows") { trip() != null }
        val cancelled = PushMessage.TrainCancelled(train, PushMessage.Reason.MISSING, leave.until)
        Notifier.show(context, cancelled, now)
        waitFor("the trip ends") { trip() == null }
        assertNotNull(manager.activeNotifications.singleOrNull { it.tag == cancelled.tag })
        assertFalse(alarmSet())
    }

    @Test
    fun cancelledWithOthersTheTrainEndsTheTripToo() {
        Notifier.show(context, leave, now)
        waitFor("the trip shows") { trip() != null }
        val other = train.copy(aimed = train.aimed.minus(Duration.ofMinutes(10)), expected = null)
        Notifier.show(context, PushMessage.TrainsCancelled(listOf(other), leave.until), now)
        SystemClock.sleep(500)
        // Another train: the trip goes on.
        assertNotNull(trip())
        Notifier.show(context, PushMessage.TrainsCancelled(listOf(other, train), leave.until), now)
        waitFor("the trip ends") { trip() == null }
        assertFalse(alarmSet())
    }

    @Test
    fun aNewReminderReplacesTheTrip() {
        Notifier.show(context, leave, now)
        waitFor("the trip shows") { trip() != null }
        SystemClock.sleep(500)
        val next = train.copy(aimed = train.aimed.plus(Duration.ofMinutes(15)), expected = train.expected!!.plus(Duration.ofMinutes(15)))
        val reminder = leave.copy(train = next, leave = leave.leave.plus(Duration.ofMinutes(15)))
        Notifier.show(context, reminder, now)
        waitFor("the new trip") { trip()?.`when` == reminder.leave.toEpochMilli() }
        assertEquals(1, manager.activeNotifications.count { it.tag == LiveTrip.TAG })
        // The first train's cancellation no longer ends it.
        Notifier.show(context, PushMessage.TrainCancelled(train, PushMessage.Reason.MISSING, leave.until), now)
        SystemClock.sleep(500)
        assertNotNull(trip())
    }
}
