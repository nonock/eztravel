package io.github.nonock.eztravel.home

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
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
                val reminded = current.reminded
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

/** A leg in its card. [onPick], on the leg the reminder is for, with why the last pick was [refused]. */
@Composable
private fun LegDepartures(leg: Departures.Leg, now: Instant, refused: Int? = null, onPick: ((Departure) -> Unit)? = null) {
    Card(
        Modifier.fillMaxWidth().padding(top = 12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
            // Read as one: "Line J, Les Aubépines → Gare Centrale".
            Row(Modifier.semantics(mergeDescendants = true) {}, verticalAlignment = Alignment.CenterVertically) {
                LineBadge(leg.line, leg.color)
                if (leg.from != null && leg.to != null) {
                    Spacer(Modifier.width(12.dp))
                    Text(stringResource(R.string.departures_leg, leg.from, leg.to), style = MaterialTheme.typography.titleSmall)
                }
            }
            refused?.let {
                Text(stringResource(it), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(top = 8.dp))
            }
            val departures = leg.departures
            when {
                departures == null -> Text(
                    stringResource(R.string.departures_unavailable),
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(top = 8.dp),
                )
                departures.isEmpty() -> Text(stringResource(R.string.departures_none), modifier = Modifier.padding(top = 8.dp))
                else -> departures.forEachIndexed { index, departure ->
                    if (index > 0) HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    DepartureRow(leg, departure, now, onPick)
                }
            }
        }
    }
}

/**
 * One departure of [leg], read as one: "07:07, +4 min, Gare Centrale, PILO,
 * Platform 2, arrives 07:23, leave in 9 min". With [onPick], a train that can
 * still be caught has a bell that picks it for the reminder to leave, and drops
 * it once picked.
 */
@Composable
private fun DepartureRow(leg: Departures.Leg, departure: Departure, now: Instant, onPick: ((Departure) -> Unit)?) {
    val pickable = onPick != null && departure.journey != null && (departure.picked || leg.catchable(departure, now))
    Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Row(Modifier.weight(1f).semantics(mergeDescendants = true) {}, verticalAlignment = Alignment.Top) {
            Column(Modifier.width(72.dp)) {
                val cancelled = departure.state == State.CANCELLED
                Text(
                    when {
                        // The metro gives no timetable: the minutes left say more.
                        departure.aimed == null -> departure.expected.minutesFrom(now).let { minutes ->
                            if (minutes == 0L) stringResource(R.string.departures_arriving) else stringResource(R.string.departures_minutes, minutes)
                        }
                        else -> departure.aimed.clock()
                    },
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                    textDecoration = if (cancelled) TextDecoration.LineThrough else null,
                )
                when {
                    cancelled -> Text(stringResource(R.string.departures_cancelled), color = MaterialTheme.colorScheme.error)
                    departure.delayMinutes > 0 -> Text(
                        stringResource(R.string.departures_late, departure.delayMinutes),
                        color = MaterialTheme.colorScheme.primary,
                        fontWeight = FontWeight.Bold,
                    )
                    departure.state == State.DELAYED -> Text(stringResource(R.string.departures_delayed), color = MaterialTheme.colorScheme.primary)
                }
            }
            Column(Modifier.weight(1f)) {
                Text(departure.destination, style = MaterialTheme.typography.bodyLarge)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    val details = MaterialTheme.typography.bodySmall
                    val muted = MaterialTheme.colorScheme.onSurfaceVariant
                    departure.platform?.let { Tag(stringResource(R.string.departures_platform, it)) }
                    departure.mission?.let { Text(it, style = details, color = muted) }
                    departure.arrival?.let { Text(stringResource(R.string.departures_arrival, it.clock()), style = details, color = muted) }
                }
                val leave = leg.leaveAt(departure)
                if (leave != null && departure.state != State.CANCELLED) {
                    val left = Duration.between(now, leave)
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
        if (pickable) Bell(departure, onPick)
    }
}

/** A small label on its own ground, such as the platform. */
@Composable
private fun Tag(text: String) {
    Surface(shape = RoundedCornerShape(4.dp), color = MaterialTheme.colorScheme.surfaceVariant) {
        Text(text, style = MaterialTheme.typography.labelMedium, modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp))
    }
}

/** Picks the train for the reminder to leave, or drops it: named by its time for TalkBack. */
@Composable
private fun Bell(departure: Departure, onPick: (Departure) -> Unit) {
    val label = stringResource(if (departure.picked) R.string.departures_unpick else R.string.departures_pick, departure.knownBy.clock())
    IconToggleButton(checked = departure.picked, onCheckedChange = { onPick(departure) }) {
        Icon(
            if (departure.picked) Icons.Filled.Notifications else Icons.Outlined.Notifications,
            contentDescription = label,
            tint = if (departure.picked) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
