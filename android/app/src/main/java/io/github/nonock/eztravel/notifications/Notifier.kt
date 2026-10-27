package io.github.nonock.eztravel.notifications

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import io.github.nonock.eztravel.MainActivity
import io.github.nonock.eztravel.R
import java.time.Duration
import java.time.Instant
import java.time.LocalDate

object Notifier {
    const val CHANNEL = "disruptions"

    /** One notification per tag: a newer message about a line replaces the older one. */
    private const val ID = 1

    private const val EXTRA_OPEN = "open"
    private const val EXTRA_LINE = "line"
    private const val EXTRA_RECORD = "record"
    private const val EXTRA_TITLE = "title"

    /** The app, opened on [opening]: started afresh, so that it shows it. */
    fun intent(context: Context, opening: Opening): Intent =
        Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra(EXTRA_OPEN, true)
            .putExtra(EXTRA_LINE, opening.line)
            .putExtra(EXTRA_RECORD, opening.record)
            .putExtra(EXTRA_TITLE, opening.title)

    /** What the app was opened on, from a notification; null otherwise. */
    fun opening(intent: Intent?): Opening? =
        intent?.takeIf { it.getBooleanExtra(EXTRA_OPEN, false) }?.let {
            Opening(it.getStringExtra(EXTRA_LINE), it.getStringExtra(EXTRA_RECORD), it.getStringExtra(EXTRA_TITLE))
        }

    fun createChannel(context: Context) {
        val channel = NotificationChannel(CHANNEL, context.getString(R.string.channel_disruptions), NotificationManager.IMPORTANCE_HIGH)
        channel.description = context.getString(R.string.channel_disruptions_description)
        context.getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    /** Shows the backend's message; false when it is not one this version knows. */
    fun receive(context: Context, data: Map<String, String>): Boolean {
        val message = PushMessage.parse(data) ?: return false
        show(context, message)
        return true
    }

    /** `now` is when the message arrived: one whose window is over is not shown. */
    fun show(context: Context, message: PushMessage, now: Instant = Instant.now()) {
        val left = message.until?.let { Duration.between(now, it).toMillis() }
        if (left != null && left <= 0) return
        createChannel(context)
        val text = message.text(context.resources)
        // Each line's own record: one pending intent per tag.
        val open = message.opening?.let { opening ->
            PendingIntent.getActivity(
                context,
                message.tag.hashCode(),
                intent(context, opening),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        } ?: PendingIntent.getActivity(context, 0, Intent(context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val builder = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setColor(ContextCompat.getColor(context, R.color.brand_amber))
            .setContentTitle(text.title)
            .setContentText(text.body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text.body))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setContentIntent(open)
            .setAutoCancel(true)
        // Nothing stale on screen once the watch window is over.
        if (left != null) builder.setTimeoutAfter(left)
        if (message is PushMessage.Tomorrow) builder.addAction(remote(context, message.date))
        post(context, message.tag, builder)
    }

    /**
     * In place of the evening notice: [date] worked from home, as [set], or
     * not changed, with its button again to try once more.
     */
    fun tomorrowSet(context: Context, date: LocalDate, set: Boolean) {
        createChannel(context)
        val (title, text) = if (set) {
            R.string.notification_tomorrow_remote to R.string.notification_tomorrow_remote_text
        } else {
            R.string.notification_tomorrow_unchanged to R.string.notification_tomorrow_unchanged_text
        }
        val builder = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setColor(ContextCompat.getColor(context, R.color.brand_amber))
            .setContentTitle(context.getString(title))
            .setContentText(context.getString(text))
            .setStyle(NotificationCompat.BigTextStyle().bigText(context.getString(text)))
            .setContentIntent(PendingIntent.getActivity(context, 0, Intent(context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE))
            .setAutoCancel(true)
        if (!set) builder.addAction(remote(context, date))
        post(context, "tomorrow", builder)
    }

    /** "Remote work tomorrow": sets [date] from the notification, without opening the app. */
    private fun remote(context: Context, date: LocalDate): NotificationCompat.Action {
        val set = PendingIntent.getBroadcast(
            context,
            "tomorrow".hashCode(),
            TomorrowReceiver.intent(context, date),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return NotificationCompat.Action(0, context.getString(R.string.notification_remote_tomorrow), set)
    }

    /** Under [tag], replacing what was there, when the user lets the app notify. */
    private fun post(context: Context, tag: String, builder: NotificationCompat.Builder) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        NotificationManagerCompat.from(context).notify(tag, ID, builder.build())
    }
}
