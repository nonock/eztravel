package io.github.nonock.eztravel.notifications

import android.content.res.Resources
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import java.time.Duration
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

data class NotificationText(val title: String, val body: String)

/**
 * The title in the phone's language; the body, for a line, says in one line
 * where the disruption is, or gives the operator's title from a backend that
 * does not say. Train times are given in [zone], the phone's own.
 */
fun PushMessage.text(resources: Resources, zone: ZoneId = ZoneId.systemDefault()): NotificationText = when (this) {
    is PushMessage.Started -> NotificationText(
        resources.getString(
            if (severity == Severity.BLOCKING) R.string.notification_started_blocking else R.string.notification_started_disrupted,
            line.label(resources),
        ),
        scope?.words(resources) ?: title ?: text.orEmpty(),
    )
    is PushMessage.Changed -> NotificationText(
        resources.getString(
            if (severity == Severity.BLOCKING) R.string.notification_changed_blocking else R.string.notification_changed_disrupted,
            line.label(resources),
        ),
        scope?.words(resources) ?: title ?: text.orEmpty(),
    )
    is PushMessage.Ended -> NotificationText(resources.getString(R.string.notification_ended, line.label(resources)), "")
    is PushMessage.Planned -> NotificationText(resources.getQuantityString(R.plurals.notification_planned, count, count), titles)
    PushMessage.Outage -> NotificationText(resources.getString(R.string.notification_outage), resources.getString(R.string.notification_outage_text))
    PushMessage.Recovered -> NotificationText(resources.getString(R.string.notification_recovered), resources.getString(R.string.notification_recovered_text))
    is PushMessage.TrainCancelled -> NotificationText(
        resources.getString(R.string.notification_train_cancelled, train.line.label(resources), train.time(resources, zone)),
        train.describe(
            resources,
            if (reason == PushMessage.Reason.ANNOUNCED) R.string.notification_train_announced else R.string.notification_train_missing,
        ),
    )
    is PushMessage.TrainBack -> NotificationText(
        resources.getString(R.string.notification_train_back, train.line.label(resources), train.time(resources, zone)),
        train.describe(resources, R.string.notification_train_route),
    )
    is PushMessage.Leave -> NotificationText(
        resources.getString(R.string.notification_leave, train.line.label(resources), leave.clock(resources, zone)),
        resources.getString(
            R.string.notification_sentences,
            if (platform == null) {
                resources.getString(R.string.notification_leave_train, train.name(resources), train.time(resources, zone), train.destination)
            } else {
                resources.getString(R.string.notification_leave_train_platform, train.name(resources), train.time(resources, zone), train.destination, platform)
            },
            when {
                !live -> resources.getString(R.string.notification_leave_scheduled)
                train.lateMinutes > 0 -> resources.getString(R.string.notification_leave_late, train.lateMinutes)
                else -> resources.getString(R.string.notification_leave_on_time)
            },
        ),
    )
    is PushMessage.TrainLate -> NotificationText(
        resources.getString(R.string.notification_train_late_title, train.line.label(resources), train.time(resources, zone)),
        resources.getString(
            R.string.notification_train_late,
            train.name(resources),
            train.destination,
            (train.expected ?: train.aimed).clock(resources, zone),
            train.lateMinutes,
        ),
    )
}

/** "Between Nanterre and Cergy", "Whole line"... */
private fun PushMessage.Scope.words(resources: Resources): String = when (this) {
    PushMessage.Scope.WholeLine -> resources.getString(R.string.notification_where_line)
    is PushMessage.Scope.Section -> resources.getString(R.string.notification_where_section, from, to)
    is PushMessage.Scope.Stops -> if (count <= 1) {
        resources.getString(R.string.notification_where_stop, first)
    } else {
        resources.getQuantityString(R.plurals.notification_where_stops, count - 1, first, count - 1)
    }
}

private fun Instant.clock(resources: Resources, zone: ZoneId): String =
    DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(resources.configuration.locales[0]).format(atZone(zone))

/** Its mission code, or just "Train". */
private fun PushMessage.Train.name(resources: Resources): String = mission ?: resources.getString(R.string.notification_train_unnamed)

/** Whole minutes late, 0 when on time or when not known. */
private val PushMessage.Train.lateMinutes: Long
    get() = expected?.let { Duration.between(aimed, it).toMinutes().coerceAtLeast(0) } ?: 0

private fun PushMessage.Train.time(resources: Resources, zone: ZoneId): String = aimed.clock(resources, zone)

/** "PALO to Gare Centrale, from Les Aubépines", in the words of [format]. */
private fun PushMessage.Train.describe(resources: Resources, format: Int): String =
    resources.getString(format, name(resources), destination, from)

fun Line.label(resources: Resources): String = resources.getString(
    when (mode) {
        Mode.TRAIN -> R.string.line_train
        Mode.METRO -> R.string.line_metro
        Mode.OTHER -> R.string.line_other
    },
    name,
)
