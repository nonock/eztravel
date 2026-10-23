package io.github.nonock.eztravel.notifications

import com.google.firebase.messaging.FirebaseMessaging

/** The phone's Firebase token, handed over once known; tests put a fake in its place. */
object PushToken {
    @Volatile
    var fetch: (onToken: (String) -> Unit) -> Unit = { onToken ->
        FirebaseMessaging.getInstance().token.addOnSuccessListener { onToken(it) }
    }
}
