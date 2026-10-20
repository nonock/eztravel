package io.github.nonock.eztravel.home

import androidx.compose.foundation.clickable
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
import androidx.compose.runtime.rememberCoroutineScope
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
import io.github.nonock.eztravel.backend.Refusal
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONException
import java.io.IOException
import java.time.Duration
import java.time.Instant

/** The backend reads each stop at most once a minute. */
private const val REFRESH_MS = 60_000L

/**
 * The next departures on each leg of the way shown, read when the screen
 * shows and every minute while it does. A train of the first leg with a time
 * to its platform can be picked for the reminder to leave.
 */
@Composable
fun NextDepartures(backend: Backend, modifier: Modifier = Modifier, now: () -> Instant = Instant::now) {
    // The time away section already says that this build has no server.
    if (!backend.configured) return
    // Null until read, or when it cannot be.
    var departures by remember { mutableStateOf<Departures?>(null) }
    var unreadable by remember { mutableStateOf(false) }
    var readAt by remember { mutableStateOf(now()) }
    // Why the last pick was not saved.
    var refused by remember { mutableStateOf<Int?>(null) }
    val scope = rememberCoroutineScope()
    // Picks the train, or drops it when it is the one picked.
    val pick: (Departure) -> Unit = { departure ->
        scope.launch {
            val journey = if (departure.picked) null else departure.journey
            try {
                val refusal = backend.pick(journey)
                refused = refusal?.message()
                if (refusal == null) departures = departures?.picking(journey)
            } catch (_: IOException) {
                refused = R.string.departures_pick_failed
            }
        }
    }
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
            else -> {
                val reminded = current.legs.firstOrNull { it.reach != null }
                for (leg in current.legs) {
                    if (leg === reminded) LegDepartures(leg, readAt, refused, pick) else LegDepartures(leg, readAt)
                }
            }
        }
    }
}

private fun Refusal.message() = when (this) {
    Refusal.TOO_LATE -> R.string.departures_refused_too_late
    Refusal.NOT_OFFICE_DAY -> R.string.departures_refused_not_office_day
    Refusal.OUTSIDE_WINDOW -> R.string.departures_refused_outside_window
    Refusal.UNKNOWN -> R.string.departures_refused_unknown
    Refusal.UNAVAILABLE -> R.string.departures_refused_unavailable
}

/** [onPick], on the leg the reminder is for, with why the last pick was [refused]. */
@Composable
private fun LegDepartures(leg: Departures.Leg, now: Instant, refused: Int? = null, onPick: ((Departure) -> Unit)? = null) {
    Spacer(Modifier.height(12.dp))
    // Read as one: "Line J, Les Aubépines → Gare Centrale".
    Row(Modifier.semantics(mergeDescendants = true) {}, verticalAlignment = Alignment.CenterVertically) {
        LineBadge(leg.line, leg.color)
        if (leg.from != null && leg.to != null) {
            Spacer(Modifier.width(12.dp))
            Text(stringResource(R.string.departures_leg, leg.from, leg.to), style = MaterialTheme.typography.titleSmall)
        }
    }
    refused?.let { Text(stringResource(it), color = MaterialTheme.colorScheme.error) }
    val departures = leg.departures
    when {
        departures == null -> Text(stringResource(R.string.departures_unavailable), color = MaterialTheme.colorScheme.error)
        departures.isEmpty() -> Text(stringResource(R.string.departures_none))
        else -> for (departure in departures) DepartureRow(departure, now, leg.reach, onPick)
    }
}

/**
 * One departure, read as one: "07:07, +4 min, Gare Centrale, PILO, Platform 2,
 * arrives 07:23, leave in 9 min". [reach] is the time to its platform, if known.
 * With [onPick], a train that can still be caught is picked by a tap, and the
 * one picked dropped.
 */
@Composable
private fun DepartureRow(departure: Departure, now: Instant, reach: Duration?, onPick: ((Departure) -> Unit)?) {
    val catchable = reach != null && departure.state != State.CANCELLED && !now.isAfter(departure.expected.minus(reach))
    val pickable = onPick != null && departure.journey != null && (departure.picked || catchable)
    val tap = if (pickable) {
        val label = stringResource(if (departure.picked) R.string.departures_unpick else R.string.departures_pick)
        Modifier.clickable(onClickLabel = label) { onPick(departure) }
    } else {
        Modifier
    }
    Row(
        Modifier.padding(top = 8.dp).then(tap).semantics(mergeDescendants = true) {},
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
            if (departure.picked) {
                Text(stringResource(R.string.departures_picked), color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
            }
        }
    }
}
