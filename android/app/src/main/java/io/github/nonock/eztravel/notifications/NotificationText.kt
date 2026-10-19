package io.github.nonock.eztravel.notifications

import android.content.res.Resources
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

data class NotificationText(val title: String, val body: String)

/**
 * The title in the phone's language; the body is the operator's text, as
 * published. Train times are given in [zone], the phone's own.
 */
fun PushMessage.text(resources: Resources, zone: ZoneId = ZoneId.systemDefault()): NotificationText = when (this) {
    is PushMessage.Started -> NotificationText(
        resources.getString(
            if (severity == Severity.BLOCKING) R.string.notification_started_blocking else R.string.notification_started_disrupted,
            line.label(resources),
        ),
        text ?: title.orEmpty(),
    )
    is PushMessage.Changed -> NotificationText(
        resources.getString(
            if (severity == Severity.BLOCKING) R.string.notification_changed_blocking else R.string.notification_changed_disrupted,
            line.label(resources),
        ),
        text ?: title.orEmpty(),
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
}

private fun PushMessage.Train.time(resources: Resources, zone: ZoneId): String =
    DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(resources.configuration.locales[0]).format(aimed.atZone(zone))

/** "PALO to Gare Centrale, from Les Aubépines", in the words of [format]. */
private fun PushMessage.Train.describe(resources: Resources, format: Int): String =
    resources.getString(format, mission ?: resources.getString(R.string.notification_train_unnamed), destination, from)

fun Line.label(resources: Resources): String = resources.getString(
    when (mode) {
        Mode.TRAIN -> R.string.line_train
        Mode.METRO -> R.string.line_metro
        Mode.OTHER -> R.string.line_other
    },
    name,
)
