package io.github.nonock.eztravel.watchdog

import android.Manifest
import android.app.AlarmManager
import android.app.PendingIntent
import android.app.job.JobInfo
import android.app.job.JobScheduler
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.PersistableBundle
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import io.github.nonock.eztravel.MainActivity
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.AppBackend
import io.github.nonock.eztravel.backend.Status
import io.github.nonock.eztravel.home.clock
import io.github.nonock.eztravel.notifications.Notifier
import org.json.JSONException
import java.io.IOException
import java.time.Duration
import java.time.Instant

/** What the phone makes of the backend at one look. */
sealed interface Finding {
    /** It checks, or says itself what goes wrong: PRIM cannot be read. */
    data object Fine : Finding

    /** It does not answer. */
    data object Unreachable : Finding

    /** It answers, but cannot tell the phone: its checks stopped, or its notifications cannot be sent. */
    data class Silent(val outage: Status.Outage) : Finding
}

/** A look's finding, and when to look again; null for no more until the app opens. */
data class Check(val finding: Finding, val next: Instant?)

/**
 * The phone looks at the backend by itself, during the watch windows: the
 * backend cannot say it stopped, nor that it cannot send. An approximate alarm
 * brings each look, with no special permission; the look itself is a job, as
 * an app brought up by an alarm has no network.
 */
object Watchdog {
    /** The first look of a window, once the backend has had time to check. */
    val AFTER_OPENING: Duration = Duration.ofMinutes(5)

    /** Looks during a window, and after a look without an answer. */
    val DURING: Duration = Duration.ofMinutes(20)

    /** Looks in a row without an answer before waiting for the app to open: two hours. */
    const val RETRIES = 6

    /** The warning's tag: one at a time, replaced, and gone once the backend is fine. */
    const val TAG = "watchdog"

    /** The look's job: one at a time, the latest in place of one waiting. */
    const val JOB = 1

    private const val ID = 1
    private const val PREFERENCES = "watchdog"
    private const val FAILURES = "failures"
    private const val NOTIFY = "notify"

    /**
     * Has the system [run] a look once the app may use a network: in the
     * background, as after an alarm, it has none.
     */
    fun lookSoon(context: Context, notify: Boolean) {
        context.getSystemService(JobScheduler::class.java).schedule(job(context, notify))
    }

    internal fun job(context: Context, notify: Boolean): JobInfo =
        JobInfo.Builder(JOB, ComponentName(context, WatchdogJob::class.java))
            .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
            .setExtras(PersistableBundle().apply { putBoolean(NOTIFY, notify) })
            // Run at once, even while the phone sleeps.
            .apply { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) setExpedited(true) }
            .build()

    /** Whether the job's look may warn. */
    internal fun notifies(extras: PersistableBundle): Boolean = extras.getBoolean(NOTIFY)

    /** [status] is null when the backend could not be read; [failures] are the looks in a row before without an answer. */
    fun check(status: Status?, now: Instant, failures: Int): Check {
        if (status == null) return Check(Finding.Unreachable, if (failures + 1 < RETRIES) now.plus(DURING) else null)
        val next = if (status.watching != null) now.plus(DURING) else status.next?.plus(AFTER_OPENING)
        val silent = status.outage?.takeIf { it.cause != Status.Cause.PRIM }
        return Check(silent?.let { Finding.Silent(it) } ?: Finding.Fine, next)
    }

    /** Sets the next look, or none; tests put a fake in its place. */
    @Volatile
    var schedule: (Context, Instant?) -> Unit = ::alarm

    private fun alarm(context: Context, at: Instant?) {
        val look = PendingIntent.getBroadcast(
            context,
            0,
            Intent(context, WatchdogReceiver::class.java).setAction(WatchdogReceiver.ACTION_LOOK),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val alarms = context.getSystemService(AlarmManager::class.java)
        if (at == null) {
            alarms.cancel(look)
            look.cancel()
        } else {
            alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at.toEpochMilli(), look)
        }
    }

    /**
     * Looks at the backend now, and sets the next look. With [notify], warns of
     * what the backend cannot say; a warning goes once the backend is fine.
     */
    suspend fun run(context: Context, notify: Boolean, now: Instant = Instant.now()) {
        val backend = AppBackend.current
        if (!backend.configured) return
        val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
        val failures = preferences.getInt(FAILURES, 0)
        val status = try {
            backend.status()
        } catch (_: IOException) {
            null
        } catch (_: JSONException) {
            null
        }
        val check = check(status, now, failures)
        preferences.edit { putInt(FAILURES, if (status == null) failures + 1 else 0) }
        when {
            check.finding == Finding.Fine -> NotificationManagerCompat.from(context).cancel(TAG, ID)
            notify -> warn(context, check.finding)
        }
        schedule(context, check.next)
    }

    /** Looks without an answer are counted afresh. */
    fun forget(context: Context) {
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit { clear() }
    }

    private fun warn(context: Context, finding: Finding) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        val (title, body) = when (finding) {
            is Finding.Silent -> Pair(
                if (finding.outage.cause == Status.Cause.FIREBASE) R.string.watchdog_cannot_send else R.string.watchdog_stopped,
                context.getString(R.string.watchdog_since, finding.outage.since.clock()),
            )
            else -> Pair(R.string.watchdog_unreachable, context.getString(R.string.watchdog_unreachable_text))
        }
        Notifier.createChannel(context)
        val open = PendingIntent.getActivity(context, 0, Intent(context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val warning = NotificationCompat.Builder(context, Notifier.CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setColor(ContextCompat.getColor(context, R.color.brand_amber))
            .setContentTitle(context.getString(title))
            .setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setContentIntent(open)
            // Looked at again every twenty minutes: said once, not at each look.
            .setOnlyAlertOnce(true)
            .build()
        NotificationManagerCompat.from(context).notify(TAG, ID, warning)
    }
}
