package io.github.nonock.eztravel.backend

import android.util.Log
import io.github.nonock.eztravel.BuildConfig
import java.io.IOException

/** The backend of this build; tests put a fake in its place. */
object AppBackend {
    @Volatile
    var current: Backend = HttpBackend(BuildConfig.BACKEND_URL, BuildConfig.APP_KEY)

    /** Blocking. On failure the phone registers again at the next launch. */
    fun register(token: String) {
        val backend = current
        if (!backend.configured) return
        try {
            backend.register(token)
        } catch (error: IOException) {
            Log.w("eztravel", "the phone could not register with the backend; next launch tries again", error)
        }
    }
}
