package io.github.nonock.eztravel.watchdog

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/**
 * The alarm's look at the backend, which may warn; after a restart or an
 * update of the app, which loses alarms, a look that only sets the next one.
 */
class WatchdogReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                Watchdog.run(context.applicationContext, notify = intent.action == ACTION_LOOK)
            } finally {
                pending.finish()
            }
        }
    }

    companion object {
        const val ACTION_LOOK = "io.github.nonock.eztravel.watchdog.LOOK"
    }
}
