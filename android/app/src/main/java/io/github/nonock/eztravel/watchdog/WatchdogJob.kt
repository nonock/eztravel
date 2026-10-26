package io.github.nonock.eztravel.watchdog

import android.app.job.JobParameters
import android.app.job.JobService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancelChildren
import kotlinx.coroutines.launch

/** A look at the backend, run by the system once the app may use a network: see [Watchdog.lookSoon]. */
class WatchdogJob : JobService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override fun onStartJob(params: JobParameters): Boolean {
        scope.launch {
            try {
                Watchdog.run(applicationContext, notify = Watchdog.notifies(params.extras))
            } finally {
                jobFinished(params, false)
            }
        }
        return true
    }

    /** Stopped, when the network goes: the look is left undone and run again once it is back. */
    override fun onStopJob(params: JobParameters): Boolean {
        scope.coroutineContext.cancelChildren()
        return true
    }
}
