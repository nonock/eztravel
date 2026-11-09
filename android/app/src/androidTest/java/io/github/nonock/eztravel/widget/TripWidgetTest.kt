package io.github.nonock.eztravel.widget

import android.Manifest
import android.app.NotificationManager
import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.content.Intent
import android.os.SystemClock
import android.view.View
import android.widget.Chronometer
import android.widget.FrameLayout
import android.widget.TextView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.GrantPermissionRule
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.notifications.LiveTrip
import io.github.nonock.eztravel.notifications.Notifier
import io.github.nonock.eztravel.notifications.PushMessage
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.clock
import io.github.nonock.eztravel.watchdog.Watchdog
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.time.Duration
import java.time.Instant
import java.time.ZoneId
import java.time.temporal.ChronoUnit
import kotlin.math.abs

@RunWith(AndroidJUnit4::class)
class TripWidgetTest {
    @get:Rule
    val permission: GrantPermissionRule = GrantPermissionRule.grant(Manifest.permission.POST_NOTIFICATIONS)

    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val now = Instant.now().truncatedTo(ChronoUnit.SECONDS)
    private val line = Line("L", Mode.TRAIN)
    private val trip = WidgetTrip(
        line,
        aimed = now.plus(Duration.ofMinutes(20)),
        departure = now.plus(Duration.ofMinutes(22)),
        platform = "2",
        leave = now.plus(Duration.ofMinutes(10)),
        until = now.plus(Duration.ofMinutes(32)),
    )

    @After
    fun clear() {
        LiveTrip.end(context)
        context.getSystemService(NotificationManager::class.java).cancelAll()
    }

    private fun shown(trip: WidgetTrip?, at: Instant): View = TripWidget.views(context, trip, at).apply(context, FrameLayout(context))

    private fun View.text(id: Int) = findViewById<TextView>(id).text.toString()

    private fun View.visible(id: Int) = findViewById<View>(id).visibility == View.VISIBLE

    private fun clock(at: Instant) = at.clock(context.resources, ZoneId.systemDefault())

    /** The countdown reaches zero [left] from when the view was made, within a second or two. */
    private fun View.countsDown(left: Duration) {
        val countdown = findViewById<Chronometer>(R.id.trip_countdown)
        assertTrue(countdown.isCountDown)
        val expected = SystemClock.elapsedRealtime() + left.toMillis()
        assertTrue("${countdown.base} vs $expected", abs(countdown.base - expected) < 2_000)
    }

    @Test
    fun beforeTheTimeToLeaveItCountsDownToIt() {
        val view = shown(trip, now)
        assertEquals(
            "L ${clock(trip.aimed)} · ${context.getString(R.string.widget_platform, "2")} · ${context.getString(R.string.widget_late, 2)}",
            view.text(R.id.trip_train),
        )
        assertEquals(context.getString(R.string.widget_leave_in), view.text(R.id.trip_label))
        view.countsDown(Duration.ofMinutes(10))
        assertFalse(view.visible(R.id.trip_idle))
    }

    @Test
    fun onceTimeToLeaveItCountsDownToTheTrain() {
        val view = shown(trip, trip.leave!!.plus(Duration.ofMinutes(1)))
        assertEquals(context.getString(R.string.widget_train_in), view.text(R.id.trip_label))
        view.countsDown(Duration.ofMinutes(11))
        // Without a time to leave, as from an older backend's late notice: the train at once.
        shown(trip.copy(leave = null), now).countsDown(Duration.ofMinutes(22))
    }

    @Test
    fun onTimeWithNoPlatformKnownItSaysTheTrainOnly() {
        assertEquals("L ${clock(trip.aimed)}", shown(trip.copy(departure = trip.aimed, platform = null), now).text(R.id.trip_train))
    }

    @Test
    fun withNoTripOrOnceTheTrainHasLeftNoneIsUnderWay() {
        for (view in listOf(shown(null, now), shown(trip, trip.departure.plus(Duration.ofMinutes(1))))) {
            assertTrue(view.visible(R.id.trip_idle))
            assertEquals(context.getString(R.string.widget_idle), view.text(R.id.trip_idle))
            assertFalse(view.visible(R.id.trip_countdown))
            assertFalse(view.visible(R.id.trip_train))
        }
    }

    @Test
    fun theWidgetKeepsTheTripAndRefreshesAtItsNextTime() {
        TripWidget.show(context, trip, now)
        assertEquals(trip, TripWidget.saved(context))
        assertTrue(TripWidget.refreshAlarm(context, PendingIntent.FLAG_NO_CREATE) != null)
        // Laid on the home screen, it shows what was kept.
        TripWidget().onUpdate(context, AppWidgetManager.getInstance(context), intArrayOf())
        // Refreshed once the train has left: nothing kept any more.
        TripWidget.refresh(context, trip.departure.plus(Duration.ofMinutes(1)))
        assertNull(TripWidget.saved(context))
        assertFalse(TripWidget.refreshAlarm(context, PendingIntent.FLAG_NO_CREATE) != null)
        // From its alarm.
        TripWidget.show(context, trip, now)
        TripWidget().onReceive(context, Intent(context, TripWidget::class.java).setAction(TripWidget.ACTION_REFRESH))
        assertEquals(trip, TripWidget.saved(context))
    }

    @Test
    fun withoutExactAlarmsTheWidgetStillRefreshes() {
        val exact = Watchdog.exactAllowed
        Watchdog.exactAllowed = { false }
        try {
            TripWidget.show(context, trip, now)
            assertTrue(TripWidget.refreshAlarm(context, PendingIntent.FLAG_NO_CREATE) != null)
        } finally {
            Watchdog.exactAllowed = exact
        }
    }

    private val train = PushMessage.Train(line, "PALO", "Gare Centrale", "Les Aubépines", trip.aimed, trip.departure)
    private val leave = PushMessage.Leave(train, trip.leave!!, "2", live = true, until = trip.until)

    @Test
    fun theReminderToLeaveGoesToTheWidget() {
        Notifier.show(context, leave, now)
        assertEquals(trip, TripWidget.saved(context))
        // From an older backend, late with no time to leave: the train, with no platform said.
        val late = train.copy(expected = train.aimed.plus(Duration.ofMinutes(5)))
        Notifier.show(context, PushMessage.TrainLate(late, trip.until), now)
        assertEquals(trip.copy(departure = late.expected!!, platform = null, leave = null), TripWidget.saved(context))
    }

    @Test
    fun theTrainCancelledEmptiesTheWidget() {
        Notifier.show(context, leave, now)
        Notifier.show(context, PushMessage.TrainCancelled(train, PushMessage.Reason.MISSING, trip.until), now)
        assertNull(TripWidget.saved(context))
    }

    @Test
    fun theWidgetFollowsTheTripEvenWithItsNotificationSwipedAway() {
        Notifier.show(context, leave, now)
        context.getSystemService(NotificationManager::class.java).cancel(LiveTrip.TAG, 1)
        SystemClock.sleep(500)
        Notifier.show(context, leave.copy(platform = "14", update = true), now)
        assertEquals(trip.copy(platform = "14"), TripWidget.saved(context))
        // Its train cancelled, it empties too.
        Notifier.show(context, PushMessage.TrainsCancelled(listOf(train), trip.until), now)
        assertNull(TripWidget.saved(context))
    }
}
