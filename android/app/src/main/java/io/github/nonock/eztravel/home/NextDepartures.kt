package io.github.nonock.eztravel.home

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Backend
import io.github.nonock.eztravel.backend.Departures
import io.github.nonock.eztravel.backend.Departures.Departure
import io.github.nonock.eztravel.backend.Departures.State
import kotlinx.coroutines.delay
import org.json.JSONException
import java.io.IOException
import java.time.Duration
import java.time.Instant

/** The backend reads each stop at most once a minute. */
private const val REFRESH_MS = 60_000L

/**
 * The next departures on each leg of the way shown, read when the screen
 * shows and every minute while it does.
 */
@Composable
fun NextDepartures(backend: Backend, modifier: Modifier = Modifier, now: () -> Instant = Instant::now) {
    // The time away section already says that this build has no server.
    if (!backend.configured) return
    // Null until read, or when it cannot be.
    var departures by remember { mutableStateOf<Departures?>(null) }
    var unreadable by remember { mutableStateOf(false) }
    var readAt by remember { mutableStateOf(now()) }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(backend) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            while (true) {
                try {
                    departures = backend.departures()
                    unreadable = false
                } catch (_: IOException) {
                    unreadable = true
                } catch (_: JSONException) {
                    unreadable = true
                }
                readAt = now()
                delay(REFRESH_MS)
            }
        }
    }

    Column(modifier) {
        Text(stringResource(R.string.departures_title), style = MaterialTheme.typography.titleMedium)
        val current = departures
        when {
            unreadable -> Text(stringResource(R.string.departures_unavailable), color = MaterialTheme.colorScheme.error)
            current == null -> Text(stringResource(R.string.away_loading))
            else -> for (leg in current.legs) LegDepartures(leg, readAt)
        }
    }
}

@Composable
private fun LegDepartures(leg: Departures.Leg, now: Instant) {
    Spacer(Modifier.height(12.dp))
    // Read as one: "Line J, Les Aubépines → Gare Centrale".
    Row(Modifier.semantics(mergeDescendants = true) {}, verticalAlignment = Alignment.CenterVertically) {
        LineBadge(leg.line, leg.color)
        if (leg.from != null && leg.to != null) {
            Spacer(Modifier.width(12.dp))
            Text(stringResource(R.string.departures_leg, leg.from, leg.to), style = MaterialTheme.typography.titleSmall)
        }
    }
    val departures = leg.departures
    when {
        departures == null -> Text(stringResource(R.string.departures_unavailable), color = MaterialTheme.colorScheme.error)
        departures.isEmpty() -> Text(stringResource(R.string.departures_none))
        else -> for (departure in departures) DepartureRow(departure, now, leg.reach)
    }
}

/**
 * One departure, read as one: "07:07, +4 min, Gare Centrale, PILO, Platform 2,
 * arrives 07:23, leave in 9 min". [reach] is the time to its platform, if known.
 */
@Composable
private fun DepartureRow(departure: Departure, now: Instant, reach: Duration?) {
    Row(
        Modifier.padding(top = 8.dp).semantics(mergeDescendants = true) {},
        verticalAlignment = Alignment.Top,
    ) {
        Column(Modifier.width(76.dp)) {
            val cancelled = departure.state == State.CANCELLED
            Text(
                when {
                    // The metro gives no timetable: the minutes left say more.
                    departure.aimed == null -> departure.expected.minutesFrom(now).let { minutes ->
                        if (minutes == 0L) stringResource(R.string.departures_arriving) else stringResource(R.string.departures_minutes, minutes)
                    }
                    else -> departure.aimed.clock()
                },
                fontWeight = FontWeight.Bold,
                textDecoration = if (cancelled) TextDecoration.LineThrough else null,
            )
            when {
                cancelled -> Text(stringResource(R.string.departures_cancelled), color = MaterialTheme.colorScheme.error)
                departure.delayMinutes > 0 -> Text(stringResource(R.string.departures_late, departure.delayMinutes), color = MaterialTheme.colorScheme.primary)
                departure.state == State.DELAYED -> Text(stringResource(R.string.departures_delayed), color = MaterialTheme.colorScheme.primary)
            }
        }
        Column(Modifier.weight(1f)) {
            Text(departure.destination)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val details = MaterialTheme.typography.bodySmall
                val muted = MaterialTheme.colorScheme.onSurfaceVariant
                departure.mission?.let { Text(it, style = details, color = muted) }
                departure.platform?.let { Text(stringResource(R.string.departures_platform, it), style = details, color = muted) }
                departure.arrival?.let { Text(stringResource(R.string.departures_arrival, it.clock()), style = details, color = muted) }
            }
            if (reach != null && departure.state != State.CANCELLED) {
                val left = Duration.between(now, departure.expected.minus(reach))
                when {
                    left.isNegative -> Text(stringResource(R.string.departures_too_late), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    left.toMinutes() == 0L -> Text(stringResource(R.string.departures_leave_now), color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
                    else -> Text(stringResource(R.string.departures_leave_in, left.toMinutes()), color = MaterialTheme.colorScheme.primary)
                }
            }
        }
    }
}
