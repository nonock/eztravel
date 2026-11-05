package io.github.nonock.eztravel.notifications

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** The alarm at the time to leave: the trip goes on the way. */
class LiveTripReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) = LiveTrip.onTheWay(context, intent)
}
