package io.github.nonock.eztravel.notifications

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import io.github.nonock.eztravel.backend.AppBackend
import io.github.nonock.eztravel.backend.CalendarDay
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.json.JSONException
import java.io.IOException
import java.time.LocalDate

/**
 * The evening notice's "Remote work tomorrow": sets the day with the backend,
 * then says in the notice's place whether it was set.
 */
class TomorrowReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val date = LocalDate.ofEpochDay(intent.getLongExtra(DATE, 0))
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                val set = try {
                    AppBackend.current.setDays(date, date, CalendarDay.Kind.REMOTE) == null
                } catch (_: IOException) {
                    false
                } catch (_: JSONException) {
                    false
                }
                Notifier.tomorrowSet(context, date, set)
            } finally {
                pending.finish()
            }
        }
    }

    companion object {
        private const val DATE = "date"

        fun intent(context: Context, date: LocalDate): Intent =
            Intent(context, TomorrowReceiver::class.java).putExtra(DATE, date.toEpochDay())
    }
}
