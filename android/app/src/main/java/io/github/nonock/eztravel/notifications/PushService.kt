package io.github.nonock.eztravel.notifications

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import io.github.nonock.eztravel.backend.AppBackend

/** Receives the backend's messages, even when the app is closed. */
class PushService : FirebaseMessagingService() {
    override fun onMessageReceived(message: RemoteMessage) {
        Notifier.receive(this, message.data)
    }

    /** Called off the main thread, so registering may block. */
    override fun onNewToken(token: String) {
        AppBackend.register(token)
    }
}
