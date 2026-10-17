package io.github.nonock.eztravel.notifications

import android.util.Log
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

/** Receives the backend's messages, even when the app is closed. */
class PushService : FirebaseMessagingService() {
    override fun onMessageReceived(message: RemoteMessage) {
        Notifier.receive(this, message.data)
    }

    override fun onNewToken(token: String) {
        // Until the backend registers devices, the token is read from the log.
        Log.i(TAG, "FCM token: $token")
    }

    companion object {
        const val TAG = "eztravel"
    }
}
