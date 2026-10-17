package io.github.nonock.eztravel.notifications

import android.Manifest
import android.app.Notification
import android.app.NotificationManager
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.GrantPermissionRule
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class NotifierTest {
    @get:Rule
    val permission: GrantPermissionRule = GrantPermissionRule.grant(Manifest.permission.POST_NOTIFICATIONS)

    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val manager = context.getSystemService(NotificationManager::class.java)
    private val started = PushMessage.Started(Line("L", Mode.TRAIN), Severity.BLOCKING, "Titre de l'opérateur", "Texte de l'opérateur")

    @After
    fun clear() = manager.cancelAll()

    private fun shown(tag: String) = manager.activeNotifications.filter { it.tag == tag }

    @Test
    fun aMessageIsShownOnTheDisruptionsChannel() {
        Notifier.show(context, started)
        val notification = shown("line-L").single().notification
        assertEquals(Notifier.CHANNEL, notification.channelId)
        assertEquals(started.text(context.resources).title, notification.extras.getString(Notification.EXTRA_TITLE))
        assertEquals("Texte de l'opérateur", notification.extras.getCharSequence(Notification.EXTRA_TEXT).toString())
    }

    @Test
    fun aLaterMessageAboutTheSameLineReplacesTheEarlierOne() {
        Notifier.show(context, started)
        Notifier.show(context, PushMessage.Ended(Line("L", Mode.TRAIN)))
        val notification = shown("line-L").single().notification
        assertEquals(PushMessage.Ended(Line("L", Mode.TRAIN)).text(context.resources).title, notification.extras.getString(Notification.EXTRA_TITLE))
    }

    @Test
    fun theChannelInterruptsTheUser() {
        Notifier.createChannel(context)
        val channel = manager.getNotificationChannel(Notifier.CHANNEL)
        assertEquals(NotificationManager.IMPORTANCE_HIGH, channel.importance)
        assertEquals(context.getString(io.github.nonock.eztravel.R.string.channel_disruptions), channel.name)
    }
}
