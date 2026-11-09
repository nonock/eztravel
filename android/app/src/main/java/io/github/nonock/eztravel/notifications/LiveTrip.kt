package io.github.nonock.eztravel.notifications

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.core.app.NotificationManagerCompat
import io.github.nonock.eztravel.watchdog.Watchdog
import io.github.nonock.eztravel.widget.TripWidget
import io.github.nonock.eztravel.widget.WidgetTrip
import java.time.Duration
import java.time.Instant

/**
 * The trip from the reminder to leave: one ongoing notification, a Live Update
 * where Android has them, counting down to the time to leave, then, from then,
 * to the train's departure. The train running late updates it; the train
 * cancelled ends it; it goes once the train has left.
 */
object LiveTrip {
    const val TAG = "trip"

    private const val ACTION_ON_THE_WAY = "io.github.nonock.eztravel.action.ON_THE_WAY"
    private const val EXTRA_TITLE = "title"
    private const val EXTRA_BODY = "body"
    private const val EXTRA_AT = "at"
    private const val EXTRA_UNTIL = "until"
    private const val EXTRA_LINE = "line"
    private const val EXTRA_AIMED = "aimed"

    /** In the notification itself: the train it is for. */
    private const val TRIP_LINE = "io.github.nonock.eztravel.trip.line"
    private const val TRIP_AIMED = "io.github.nonock.eztravel.trip.aimed"

    /** What the trip shows: [text], counting down to [at], until [until]; for the train of [line] at [aimed]. */
    private data class Trip(val text: NotificationText, val at: Instant, val until: Instant?, val line: String, val aimed: Instant)

    /** The trip for this train: it goes once the train has left, as the widget does, and by [until] at the latest. */
    private fun PushMessage.Train.trip(text: NotificationText, at: Instant, until: Instant?) =
        Trip(text, at, listOfNotNull(expected ?: aimed, until).min(), line.name, aimed)

    private fun onTheWay(context: Context, leave: PushMessage.Leave) =
        leave.train.trip(leave.onTheWay(context.resources), leave.train.expected ?: leave.train.aimed, leave.until)

    /**
     * The reminder to leave, at [now]: until the time to leave, counting down to
     * it, then to the train. An update changes the trip quietly, if still shown.
     */
    fun leave(context: Context, leave: PushMessage.Leave, now: Instant) {
        // The widget follows the trip, even with its notification swiped away.
        val train = leave.train
        TripWidget.show(context, WidgetTrip(train.line, train.aimed, train.expected ?: train.aimed, leave.platform, leave.leave, leave.until), now)
        if (leave.update && shown(context)?.isFor(leave.train.line.name, leave.train.aimed) != true) return
        val onTheWay = onTheWay(context, leave)
        if (now < leave.leave) {
            post(context, leave.train.trip(leave.text(context.resources), leave.leave, leave.until), now, quietly = leave.update)
            schedule(context, leave.leave, onTheWay)
        } else {
            cancelAlarm(context)
            post(context, onTheWay, now, quietly = leave.update)
        }
    }

    /**
     * The trip's train runs late: a reminder with the new time to leave, or,
     * from an older backend, counting down to its new departure.
     */
    fun late(context: Context, late: PushMessage.TrainLate, now: Instant) {
        late.leave?.let {
            return leave(context, PushMessage.Leave(late.train, it, late.platform, live = true, until = late.until), now)
        }
        val train = late.train
        TripWidget.show(context, WidgetTrip(train.line, train.aimed, train.expected ?: train.aimed, null, null, late.until), now)
        cancelAlarm(context)
        post(context, late.train.trip(late.text(context.resources), late.train.expected ?: late.train.aimed, late.until), now, quietly = false)
    }

    /** Ends the trip if it is for one of [trains], cancelled: as notified, or on the widget. */
    fun cancelled(context: Context, trains: List<PushMessage.Train>) {
        val shown = shown(context)
        val widget = TripWidget.saved(context)
        val ends = trains.any { train ->
            shown?.isFor(train.line.name, train.aimed) == true || (widget?.line?.name == train.line.name && widget.aimed == train.aimed)
        }
        if (ends) end(context)
    }

    /** At the time to leave, from the alarm's [intent]: the trip on the way, unless it was dismissed or replaced. */
    fun onTheWay(context: Context, intent: Intent, now: Instant = Instant.now()) {
        val trip = trip(intent) ?: return
        if (shown(context)?.isFor(trip.line, trip.aimed) != true) return
        post(context, trip, now, quietly = true)
    }

    /** No trip shown, nor on its way, nor on the widget. */
    fun end(context: Context) {
        NotificationManagerCompat.from(context).cancel(TAG, Notifier.ID)
        cancelAlarm(context)
        TripWidget.clear(context)
    }

    /** What the alarm at the time to leave of [leave] carries. */
    internal fun onTheWayIntent(context: Context, leave: PushMessage.Leave): Intent = intent(context, onTheWay(context, leave))

    /** The alarm to go on the way, carrying [intent] or any: with [PendingIntent.FLAG_NO_CREATE], null when none is set. */
    internal fun alarm(context: Context, intent: Intent?, flags: Int): PendingIntent? = PendingIntent.getBroadcast(
        context,
        0,
        intent ?: Intent(context, LiveTripReceiver::class.java).setAction(ACTION_ON_THE_WAY),
        flags or PendingIntent.FLAG_IMMUTABLE,
    )

    private fun shown(context: Context): Notification? =
        context.getSystemService(NotificationManager::class.java).activeNotifications.firstOrNull { it.tag == TAG }?.notification

    private fun Notification.isFor(line: String, aimed: Instant) =
        extras.getString(TRIP_LINE) == line && extras.getLong(TRIP_AIMED) == aimed.toEpochMilli()

    private fun post(context: Context, trip: Trip, now: Instant, quietly: Boolean) {
        val left = trip.until?.let { Duration.between(now, it).toMillis() }
        if (left != null && left <= 0) return end(context)
        Notifier.createChannel(context)
        val builder = Notifier.builder(context, trip.text, Notifier.commute(context))
            .setOngoing(true)
            .setAutoCancel(false)
            .setOnlyAlertOnce(quietly)
            .setWhen(trip.at.toEpochMilli())
            .setShowWhen(true)
            .setUsesChronometer(true)
            .setChronometerCountDown(true)
            .setRequestPromotedOngoing(true)
            .addExtras(Bundle().apply {
                putString(TRIP_LINE, trip.line)
                putLong(TRIP_AIMED, trip.aimed.toEpochMilli())
            })
        if (left != null) builder.setTimeoutAfter(left)
        Notifier.post(context, TAG, builder)
    }

    private fun intent(context: Context, trip: Trip): Intent =
        Intent(context, LiveTripReceiver::class.java)
            .setAction(ACTION_ON_THE_WAY)
            .putExtra(EXTRA_TITLE, trip.text.title)
            .putExtra(EXTRA_BODY, trip.text.body)
            .putExtra(EXTRA_AT, trip.at.toEpochMilli())
            .putExtra(EXTRA_UNTIL, trip.until?.toEpochMilli() ?: -1L)
            .putExtra(EXTRA_LINE, trip.line)
            .putExtra(EXTRA_AIMED, trip.aimed.toEpochMilli())

    private fun trip(intent: Intent): Trip? {
        val until = intent.getLongExtra(EXTRA_UNTIL, -1L)
        return Trip(
            NotificationText(intent.getStringExtra(EXTRA_TITLE) ?: return null, intent.getStringExtra(EXTRA_BODY) ?: return null),
            Instant.ofEpochMilli(intent.getLongExtra(EXTRA_AT, -1L).takeIf { it >= 0 } ?: return null),
            if (until >= 0) Instant.ofEpochMilli(until) else null,
            intent.getStringExtra(EXTRA_LINE) ?: return null,
            Instant.ofEpochMilli(intent.getLongExtra(EXTRA_AIMED, -1L).takeIf { it >= 0 } ?: return null),
        )
    }

    private fun schedule(context: Context, at: Instant, trip: Trip) {
        val alarm = alarm(context, intent(context, trip), PendingIntent.FLAG_UPDATE_CURRENT)!!
        val alarms = context.getSystemService(AlarmManager::class.java)
        if (Watchdog.exactAllowed(context)) {
            alarms.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at.toEpochMilli(), alarm)
        } else {
            // The phone may bring it a little late.
            alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at.toEpochMilli(), alarm)
        }
    }

    private fun cancelAlarm(context: Context) {
        val alarm = alarm(context, null, PendingIntent.FLAG_NO_CREATE) ?: return
        context.getSystemService(AlarmManager::class.java).cancel(alarm)
        alarm.cancel()
    }
}
