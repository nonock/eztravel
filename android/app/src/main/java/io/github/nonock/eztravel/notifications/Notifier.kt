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

object Notifier {
    const val CHANNEL = "disruptions"

    /** One notification per tag: a newer message about a line replaces the older one. */
    private const val ID = 1

    fun createChannel(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
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

    fun show(context: Context, message: PushMessage) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        createChannel(context)
        val text = message.text(context.resources)
        val open = PendingIntent.getActivity(context, 0, Intent(context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val notification = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(text.title)
            .setContentText(text.body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text.body))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setContentIntent(open)
            .setAutoCancel(true)
            .build()
        NotificationManagerCompat.from(context).notify(message.tag, ID, notification)
    }
}
