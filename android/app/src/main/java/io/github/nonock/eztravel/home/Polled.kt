package io.github.nonock.eztravel.home

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.delay
import org.json.JSONException
import java.io.IOException

/** What a screen read from the backend: not yet, nothing it can show, or what it gave. */
sealed interface Reading<out T> {
    data object Loading : Reading<Nothing>

    /** The backend did not answer, answered garbled, or said it could not tell. */
    data object Failed : Reading<Nothing>

    data class Known<T>(val value: T) : Reading<T>
}

/**
 * [read] when the screen shows, every [everyMs] while it does, and again when
 * one of [keys] changes; [onRead] follows each read, readable or not. A read
 * that gives null failed: it is never taken as "nothing to say". The screen
 * may put what it changed itself in the state, until the next read.
 */
@Composable
fun <T : Any> polled(
    vararg keys: Any?,
    everyMs: Long,
    onRead: () -> Unit = {},
    read: suspend () -> T?,
): MutableState<Reading<T>> {
    val reading = remember { mutableStateOf<Reading<T>>(Reading.Loading) }
    val latestRead by rememberUpdatedState(read)
    val latestOnRead by rememberUpdatedState(onRead)
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(*keys) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            while (true) {
                reading.value = try {
                    latestRead()?.let { Reading.Known(it) } ?: Reading.Failed
                } catch (_: IOException) {
                    Reading.Failed
                } catch (_: JSONException) {
                    Reading.Failed
                }
                latestOnRead()
                delay(everyMs)
            }
        }
    }
    return reading
}
