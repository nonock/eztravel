package io.github.nonock.eztravel.home

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Backend
import io.github.nonock.eztravel.backend.Status
import io.github.nonock.eztravel.backend.Status.Cause
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import io.github.nonock.eztravel.notifications.label
import kotlinx.coroutines.delay
import org.json.JSONException
import java.io.IOException

/** Checks run every two minutes: reading every minute keeps the screen current. */
private const val REFRESH_MS = 60_000L

private sealed interface Seen {
    data object Loading : Seen
    data object Unreachable : Seen
    data class Known(val status: Status) : Seen
}

/**
 * What the backend sees: the lines being watched, and whether checks work.
 * Read when the screen shows, every minute while it does, and when `refresh` changes.
 */
@Composable
fun CommuteStatus(backend: Backend, refresh: Int, modifier: Modifier = Modifier) {
    // The time away section already says that this build has no server.
    if (!backend.configured) return
    var seen by remember { mutableStateOf<Seen>(Seen.Loading) }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(backend, refresh) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            while (true) {
                seen = try {
                    Seen.Known(backend.status())
                } catch (_: IOException) {
                    Seen.Unreachable
                } catch (_: JSONException) {
                    Seen.Unreachable
                }
                delay(REFRESH_MS)
            }
        }
    }

    Column(modifier) {
        Text(stringResource(R.string.status_title), style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        when (val current = seen) {
            Seen.Loading -> Text(stringResource(R.string.away_loading))
            Seen.Unreachable -> Text(stringResource(R.string.status_unreachable), color = MaterialTheme.colorScheme.error)
            is Seen.Known -> Known(current.status)
        }
    }
}

@Composable
private fun Known(status: Status) {
    val resources = LocalResources.current
    status.outage?.let { outage ->
        val reason = when (outage.cause) {
            Cause.PRIM -> R.string.status_outage_prim
            Cause.FIREBASE -> R.string.status_outage_firebase
            Cause.SERVER -> R.string.status_outage_server
        }
        Text(stringResource(reason, outage.since.shown()), color = MaterialTheme.colorScheme.error)
        Spacer(Modifier.height(8.dp))
    }
    when (status.watching) {
        null -> {
            Text(stringResource(R.string.status_not_watching))
            Text(status.next?.let { stringResource(R.string.status_next, it.shown()) } ?: stringResource(R.string.status_no_next))
        }
        else -> {
            Text(
                stringResource(
                    if (status.watching == Direction.OUTBOUND) R.string.status_watching_outbound else R.string.status_watching_inbound,
                ),
            )
            for (state in status.lines) {
                val severity = when (state.severity) {
                    null -> R.string.status_line_normal
                    Severity.BLOCKING -> R.string.status_line_blocking
                    Severity.DISRUPTED -> R.string.status_line_disrupted
                }
                Text(stringResource(severity, state.line.label(resources)))
            }
        }
    }
    status.checkedAt?.let { Text(stringResource(R.string.status_checked, it.shown())) }
    if (status.planned.isNotEmpty()) {
        Spacer(Modifier.height(8.dp))
        Text(stringResource(R.string.status_planned))
        for (title in status.planned) Text(title)
    }
}
