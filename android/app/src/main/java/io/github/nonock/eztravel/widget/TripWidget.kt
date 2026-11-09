package io.github.nonock.eztravel.widget

import android.app.AlarmManager
import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.SystemClock
import android.view.View
import android.widget.RemoteViews
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.notifications.Notifier
import io.github.nonock.eztravel.notifications.PushMessage
import io.github.nonock.eztravel.notifications.clock
import io.github.nonock.eztravel.watchdog.Watchdog
import java.time.Duration
import java.time.Instant
import java.time.ZoneId

/**
 * The trip the widget shows: [line]'s train of [aimed], leaving at
 * [departure], from [platform] when known; time to [leave] for it, when said;
 * gone [until] at the latest.
 */
data class WidgetTrip(
    val line: PushMessage.Line,
    val aimed: Instant,
    val departure: Instant,
    val platform: String?,
    val leave: Instant?,
    val until: Instant?,
)

/**
 * The trip from the reminder to leave, on the home screen: counting down to the
 * time to leave, then to the train, with the train, its platform and delay. It
 * keeps the trip, refreshes itself when the time to leave comes and when the
 * train leaves, and then no trip is under way. A tap opens the commute.
 */
class TripWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        manager.updateAppWidget(ids, views(context, saved(context), Instant.now()))
    }

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == ACTION_REFRESH) refresh(context) else super.onReceive(context, intent)
    }

    companion object {
        const val ACTION_REFRESH = "io.github.nonock.eztravel.action.REFRESH_TRIP"

        private const val PREFERENCES = "trip_widget"
        private const val LINE = "line"
        private const val MODE = "mode"
        private const val AIMED = "aimed"
        private const val DEPARTURE = "departure"
        private const val PLATFORM = "platform"
        private const val LEAVE = "leave"
        private const val UNTIL = "until"

        /** Shows [trip] from [now] on, and keeps it. */
        fun show(context: Context, trip: WidgetTrip, now: Instant) {
            preferences(context).edit()
                .clear()
                .putString(LINE, trip.line.name)
                .putString(MODE, trip.line.mode.name)
                .putLong(AIMED, trip.aimed.toEpochMilli())
                .putLong(DEPARTURE, trip.departure.toEpochMilli())
                .putString(PLATFORM, trip.platform)
                .putLong(LEAVE, trip.leave?.toEpochMilli() ?: -1L)
                .putLong(UNTIL, trip.until?.toEpochMilli() ?: -1L)
                .commit()
            refresh(context, now)
        }

        /** No trip under way. */
        fun clear(context: Context) {
            preferences(context).edit().clear().commit()
            refresh(context)
        }

        /** Shows the trip kept as it is at [now], and sets the next refresh; a trip over is forgotten. */
        fun refresh(context: Context, now: Instant = Instant.now()) {
            val trip = saved(context)
            val next = trip?.let { next(it, now) }
            if (trip != null && next == null) preferences(context).edit().clear().commit()
            val widgets = AppWidgetManager.getInstance(context)
            widgets.updateAppWidget(widgets.getAppWidgetIds(ComponentName(context, TripWidget::class.java)), views(context, trip, now))
            val alarms = context.getSystemService(AlarmManager::class.java)
            if (next == null) {
                refreshAlarm(context, PendingIntent.FLAG_NO_CREATE)?.let {
                    alarms.cancel(it)
                    it.cancel()
                }
            } else if (Watchdog.exactAllowed(context)) {
                alarms.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, next.toEpochMilli(), refreshAlarm(context, PendingIntent.FLAG_UPDATE_CURRENT)!!)
            } else {
                alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, next.toEpochMilli(), refreshAlarm(context, PendingIntent.FLAG_UPDATE_CURRENT)!!)
            }
        }

        /** The trip kept, or none. */
        fun saved(context: Context): WidgetTrip? {
            val kept = preferences(context)
            val instant = { key: String -> kept.getLong(key, -1L).takeIf { it >= 0 }?.let(Instant::ofEpochMilli) }
            return WidgetTrip(
                PushMessage.Line(
                    kept.getString(LINE, null) ?: return null,
                    runCatching { PushMessage.Mode.valueOf(kept.getString(MODE, null)!!) }.getOrDefault(PushMessage.Mode.OTHER),
                ),
                instant(AIMED) ?: return null,
                instant(DEPARTURE) ?: return null,
                kept.getString(PLATFORM, null),
                instant(LEAVE),
                instant(UNTIL),
            )
        }

        /** The widget showing [trip] at [now], or that none is under way. */
        fun views(context: Context, trip: WidgetTrip?, now: Instant): RemoteViews {
            val views = RemoteViews(context.packageName, R.layout.widget_trip)
            views.setOnClickPendingIntent(R.id.trip_root, Notifier.commute(context))
            val shown = trip?.takeIf { next(it, now) != null }
            for (id in listOf(R.id.trip_train, R.id.trip_label, R.id.trip_countdown)) {
                views.setViewVisibility(id, if (shown == null) View.GONE else View.VISIBLE)
            }
            views.setViewVisibility(R.id.trip_idle, if (shown == null) View.VISIBLE else View.GONE)
            if (shown == null) return views
            val leaving = shown.leave?.takeIf { now < it }
            views.setTextViewText(R.id.trip_train, describe(context, shown))
            views.setTextViewText(R.id.trip_label, context.getString(if (leaving != null) R.string.widget_leave_in else R.string.widget_train_in))
            val left = Duration.between(now, leaving ?: shown.departure).toMillis()
            views.setChronometer(R.id.trip_countdown, SystemClock.elapsedRealtime() + left, null, true)
            views.setChronometerCountDown(R.id.trip_countdown, true)
            return views
        }

        /** The alarm to refresh the widget: with [PendingIntent.FLAG_NO_CREATE], null when none is set. */
        internal fun refreshAlarm(context: Context, flags: Int): PendingIntent? = PendingIntent.getBroadcast(
            context,
            0,
            Intent(context, TripWidget::class.java).setAction(ACTION_REFRESH),
            flags or PendingIntent.FLAG_IMMUTABLE,
        )

        /** When the widget next changes: the time to leave, then the train leaving, within the trip's end; none once over. */
        private fun next(trip: WidgetTrip, now: Instant): Instant? {
            val over = listOfNotNull(trip.departure, trip.until).min()
            return listOfNotNull(trip.leave?.takeIf { it < over }, over).firstOrNull { now < it }
        }

        /** "L 7:52 · platform 2 · +2 min". */
        private fun describe(context: Context, trip: WidgetTrip): String {
            val parts = mutableListOf(context.getString(R.string.widget_train, trip.line.name, trip.aimed.clock(context.resources, ZoneId.systemDefault())))
            trip.platform?.let { parts += context.getString(R.string.widget_platform, it) }
            val late = Duration.between(trip.aimed, trip.departure).toMinutes()
            if (late > 0) parts += context.getString(R.string.widget_late, late.toInt())
            return parts.joinToString(" · ")
        }

        /**
         * Where the trip is kept, written with `commit`, at once: a receiver's
         * process may be killed right after, and a trip over must not come back.
         */
        private fun preferences(context: Context) = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
    }
}
