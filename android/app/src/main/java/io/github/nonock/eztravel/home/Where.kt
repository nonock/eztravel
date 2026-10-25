package io.github.nonock.eztravel.home

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.location.LocationManager
import android.os.Build
import android.os.CancellationSignal
import androidx.core.content.ContextCompat
import androidx.core.location.LocationManagerCompat
import io.github.nonock.eztravel.backend.Position
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeoutOrNull
import kotlin.coroutines.resume

/** A position takes a few seconds at most: past that, the way's order is good enough. */
private const val LOCATE_MS = 5_000L

/**
 * Where the phone is, roughly, when [allowed]: none otherwise, or when it
 * cannot tell soon. Asked while the app shows only, and kept on the phone.
 */
@SuppressLint("MissingPermission") // Checked by [allowed].
suspend fun Context.position(
    allowed: Boolean = checkSelfPermission(Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED,
): Position? {
    if (!allowed) return null
    val manager = getSystemService(LocationManager::class.java)
    val provider = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) LocationManager.FUSED_PROVIDER else LocationManager.NETWORK_PROVIDER
    if (!manager.isProviderEnabled(provider)) return null
    return withTimeoutOrNull(LOCATE_MS) {
        suspendCancellableCoroutine { done ->
            val signal = CancellationSignal()
            done.invokeOnCancellation { signal.cancel() }
            LocationManagerCompat.getCurrentLocation(manager, provider, signal, ContextCompat.getMainExecutor(this@position)) { location ->
                done.resume(location?.let { Position(it.latitude, it.longitude) })
            }
        }
    }
}
