package io.github.nonock.eztravel.watchdog

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * The alarm's look at the backend, which may warn; after a restart or an
 * update of the app, which loses alarms, or once exact alarms are allowed, a
 * look that only sets the next one.
 */
class WatchdogReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        Watchdog.lookSoon(context.applicationContext, notify = intent.action == ACTION_LOOK)
    }

    companion object {
        const val ACTION_LOOK = "io.github.nonock.eztravel.watchdog.LOOK"
    }
}
