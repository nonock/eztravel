package io.github.nonock.eztravel.notifications

import android.Manifest
import android.app.Notification
import android.app.NotificationManager
import android.os.SystemClock
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.GrantPermissionRule
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import org.junit.After
import android.app.PendingIntent
import android.content.Intent
import io.github.nonock.eztravel.MainActivity
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.time.Duration
import java.time.Instant

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

    private fun titleOf(tag: String) = shown(tag).singleOrNull()?.notification?.extras?.getString(Notification.EXTRA_TITLE)

    /** Posting is asynchronous and limited to a few updates per second per app: wait for the system. */
    private fun waitForTitle(tag: String, expected: String) {
        val deadline = SystemClock.uptimeMillis() + 3_000
        while (titleOf(tag) != expected && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(50)
        assertEquals(expected, titleOf(tag))
    }

    @Test
    fun aMessageIsShownOnTheDisruptionsChannel() {
        Notifier.show(context, started)
        waitForTitle("line-L", started.text(context.resources).title)
        val notification = shown("line-L").single().notification
        assertEquals(Notifier.CHANNEL, notification.channelId)
        // The logo's amber, for the icon in the notification shade.
        assertEquals(context.getColor(io.github.nonock.eztravel.R.color.brand_amber), notification.color)
        // From a backend that does not say where: the operator's title, in one line.
        assertEquals("Titre de l'opérateur", notification.extras.getCharSequence(Notification.EXTRA_TEXT).toString())
    }

    @Test
    fun aLineNotificationOpensTheAppOnItsRecord() {
        val opening = Opening("L", "incident-l", "Ligne L : trafic interrompu")
        val intent = Notifier.intent(context, opening)
        assertEquals(opening, Notifier.opening(intent))
        // Started afresh, so that it shows what was tapped.
        assertTrue(intent.flags and Intent.FLAG_ACTIVITY_CLEAR_TASK != 0)
        // Opened from its icon, the app opens nothing in particular.
        assertNull(Notifier.opening(Intent(context, MainActivity::class.java)))
        assertNull(Notifier.opening(null))
    }

    @Test
    fun theWatchAndTrainsOpenTheCommute() {
        Notifier.show(context, PushMessage.Outage)
        waitForTitle("watch", PushMessage.Outage.text(context.resources).title)
        val commute = PendingIntent.getActivity(context, 0, Intent(context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        assertEquals(commute, shown("watch").single().notification.contentIntent)
    }

    @Test
    fun eachLineNotificationOpensItsOwnRecord() {
        Notifier.show(context, started)
        val metro = PushMessage.Started(Line("X", Mode.METRO), Severity.DISRUPTED, "Autre titre", null)
        Notifier.show(context, metro)
        waitForTitle("line-L", started.text(context.resources).title)
        waitForTitle("line-X", metro.text(context.resources).title)
        assertNotEquals(shown("line-L").single().notification.contentIntent, shown("line-X").single().notification.contentIntent)
    }

    @Test
    fun aLineMessageGoesAwayWhenTheWindowEnds() {
        val now = Instant.now()
        Notifier.show(context, started.copy(until = now.plus(Duration.ofMinutes(20))), now)
        waitForTitle("line-L", started.text(context.resources).title)
        assertEquals(Duration.ofMinutes(20).toMillis(), shown("line-L").single().notification.timeoutAfter)
    }

    @Test
    fun aLineMessageArrivingAfterItsWindowIsNotShown() {
        // Held by Firebase while the phone was offline: it is stale by now.
        val now = Instant.now()
        Notifier.show(context, started.copy(until = now.minus(Duration.ofMinutes(1))), now)
        SystemClock.sleep(500)
        assertEquals(emptyList<Any>(), shown("line-L"))
    }

    @Test
    fun aLaterMessageAboutTheSameLineReplacesTheEarlierOne() {
        Notifier.show(context, started)
        waitForTitle("line-L", started.text(context.resources).title)
        SystemClock.sleep(500)
        val ended = PushMessage.Ended(Line("L", Mode.TRAIN))
        Notifier.show(context, ended)
        waitForTitle("line-L", ended.text(context.resources).title)
        assertEquals(1, shown("line-L").size)
    }

    @Test
    fun aReceivedMessageIsShown() {
        val data = mapOf("type" to "started", "line" to "L", "mode" to "train", "severity" to "blocking", "text" to "Texte reçu")
        assertEquals(true, Notifier.receive(context, data))
        waitForTitle("line-L", PushMessage.parse(data)!!.text(context.resources).title)
        assertEquals("Texte reçu", shown("line-L").single().notification.extras.getCharSequence(Notification.EXTRA_TEXT).toString())
    }

    @Test
    fun anUnknownMessageIsDropped() {
        assertEquals(false, Notifier.receive(context, mapOf("type" to "something-new")))
        assertEquals(0, manager.activeNotifications.size)
    }

    @Test
    fun theChannelInterruptsTheUser() {
        Notifier.createChannel(context)
        val channel = manager.getNotificationChannel(Notifier.CHANNEL)
        assertEquals(NotificationManager.IMPORTANCE_HIGH, channel.importance)
        assertEquals(context.getString(io.github.nonock.eztravel.R.string.channel_disruptions), channel.name)
    }
}
