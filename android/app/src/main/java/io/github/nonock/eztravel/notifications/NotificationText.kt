package io.github.nonock.eztravel.notifications

import android.content.res.Resources
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity

data class NotificationText(val title: String, val body: String)

/** The title in the phone's language; the body is the operator's text, as published. */
fun PushMessage.text(resources: Resources): NotificationText = when (this) {
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
}

fun Line.label(resources: Resources): String = resources.getString(
    when (mode) {
        Mode.TRAIN -> R.string.line_train
        Mode.METRO -> R.string.line_metro
        Mode.OTHER -> R.string.line_other
    },
    name,
)
