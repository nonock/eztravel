package io.github.nonock.eztravel.home

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Backend
import io.github.nonock.eztravel.backend.Departures
import io.github.nonock.eztravel.backend.Departures.Departure
import io.github.nonock.eztravel.backend.Departures.State
import io.github.nonock.eztravel.backend.Position
import io.github.nonock.eztravel.backend.Refusal
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.io.IOException
import java.time.Duration
import java.time.Instant

/** The backend reads each stop at most once a minute. */
private const val REFRESH_MS = 60_000L

/** The countdowns move between reads. */
private const val TICK_MS = 15_000L

/**
 * The next departures on each leg of the way shown, read when the screen
 * shows and every minute while it does, the train to catch first and in big.
 * A train of the first leg with a time to its platform can be picked for the
 * reminder to leave. The countdowns move every [tickMs]. Read again when
 * [refresh] changes; [onRead] follows each read, readable or not.
 */
@Composable
fun NextDepartures(
    backend: Backend,
    modifier: Modifier = Modifier,
    now: () -> Instant = Instant::now,
    tickMs: Long = TICK_MS,
    refresh: Int = 0,
    onRead: () -> Unit = {},
    where: suspend () -> Position? = phoneWhere(),
) {
    // The commute tab already says that this build has no server.
    if (!backend.configured) return
    var clock by remember { mutableStateOf(now()) }
    val read = polled(backend, refresh, everyMs = REFRESH_MS, onRead = { onRead(); clock = now() }) { backend.departures() }
    // Where the phone is, read on its own: the departures do not wait for it.
    val here = (polled(refresh, everyMs = REFRESH_MS) { where() }.value as? Reading.Known)?.value
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
                // Shown picked at once, until the next read says so too.
                val known = read.value
                if (refusal == null && known is Reading.Known) read.value = Reading.Known(known.value.picking(journey))
            } catch (_: IOException) {
                refused = R.string.departures_pick_failed
            }
        }
    }
    LaunchedEffect(tickMs) {
        while (true) {
            delay(tickMs)
            clock = now()
        }
    }

    Column(modifier) {
        val current = read.value
        val featured = (current as? Reading.Known)?.value?.featured(clock)
        featured?.let { NextTrain(it, clock) }
        Text(
            stringResource(R.string.departures_title),
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.padding(top = if (featured == null) 0.dp else 24.dp),
        )
        when (current) {
            Reading.Failed -> Text(stringResource(R.string.departures_unavailable), color = MaterialTheme.colorScheme.error)
            Reading.Loading -> Text(stringResource(R.string.loading))
            is Reading.Known -> {
                val reminded = current.value.reminded
                // The ride nearest the phone first: at the station of the second ride, it comes first.
                for (leg in current.value.nearestFirst(here)) {
                    if (leg === reminded) LegDepartures(leg, clock, refused, pick) else LegDepartures(leg, clock)
                }
                for (backup in current.value.backups) BackupRoute(backup, clock)
            }
        }
    }
}

/**
 * The train to catch, picked or next, in big: when to leave for it, read as
 * one. Raised navy with an amber edge, as a platform's painted edge: amber is
 * kept for what to act on, and the lines' own colors stay readable on it.
 */
@Composable
private fun NextTrain(featured: Departures.Featured, now: Instant) {
    val departure = featured.departure
    Card(
        Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
    ) {
        Row(Modifier.height(IntrinsicSize.Min)) {
            Box(Modifier.width(6.dp).fillMaxHeight().background(MaterialTheme.colorScheme.primary))
            NextTrainContent(featured, departure, now)
        }
    }
}

@Composable
private fun NextTrainContent(featured: Departures.Featured, departure: Departure, now: Instant) {
    Column(Modifier.padding(16.dp).semantics(mergeDescendants = true) {}) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (departure.picked) {
                Icon(Icons.Filled.Notifications, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
            }
            Text(
                stringResource(
                    when {
                        departure.picked -> R.string.departures_hero_picked
                        departure.usual -> R.string.departures_hero_usual
                        else -> R.string.departures_hero_next
                    },
                ),
                style = MaterialTheme.typography.labelLarge,
            )
        }
        Row(Modifier.padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            LineBadge(featured.leg.line, featured.leg.color)
            Spacer(Modifier.width(12.dp))
            Text(departure.knownBy.clock(), style = MaterialTheme.typography.headlineSmall.tabular(), fontWeight = FontWeight.Bold)
            if (departure.delayMinutes > 0) {
                Spacer(Modifier.width(8.dp))
                Text(
                    stringResource(R.string.departures_late, departure.delayMinutes),
                    color = MaterialTheme.colorScheme.primary,
                    fontWeight = FontWeight.Bold,
                )
            }
            Spacer(Modifier.width(12.dp))
            Text(
                departure.destination,
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            departure.platform?.let { Tag(stringResource(R.string.departures_platform, it)) }
        }
        val left = Duration.between(now, featured.leave)
        Row(Modifier.padding(top = 8.dp), verticalAlignment = Alignment.Bottom) {
            when {
                // Most likely on the way to it.
                left.isNegative -> Text(leavesIn(departure, now), style = MaterialTheme.typography.titleLarge)
                left.toMinutes() == 0L -> Text(
                    stringResource(R.string.departures_hero_now),
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.Bold,
                    color = MaterialTheme.colorScheme.primary,
                )
                else -> {
                    Text(stringResource(R.string.departures_hero_leave_in), style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.width(8.dp))
                    Text(
                        stringResource(R.string.departures_minutes, left.toMinutes()),
                        style = MaterialTheme.typography.displaySmall.tabular(),
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
            }
        }
    }
}

/** Where the phone is, as this app's context tells. */
@Composable
private fun phoneWhere(): suspend () -> Position? {
    val context = LocalContext.current.applicationContext
    return { context.position() }
}

/** When [departure] leaves: in so many minutes, or now. */
@Composable
private fun leavesIn(departure: Departure, now: Instant): String {
    val minutes = departure.expected.minutesFrom(now)
    return if (minutes == 0L) stringResource(R.string.departures_leaving) else stringResource(R.string.departures_leaves_in, minutes)
}

/** Figures of one width, so that a countdown does not jitter as it moves. */
private fun TextStyle.tabular() = copy(fontFeatureSettings = "tnum")

private fun Refusal.message() = when (this) {
    Refusal.TOO_LATE -> R.string.departures_refused_too_late
    Refusal.NOT_OFFICE_DAY -> R.string.departures_refused_not_office_day
    Refusal.OUTSIDE_WINDOW -> R.string.departures_refused_outside_window
    Refusal.UNKNOWN -> R.string.departures_refused_unknown
    Refusal.UNAVAILABLE -> R.string.departures_refused_unavailable
}

/** A backup needed now: the lines of the way it stands in for, as they run, then its rides. */
@Composable
private fun BackupRoute(backup: Departures.Backup, now: Instant) {
    Text(
        stringResource(R.string.backup_title),
        style = MaterialTheme.typography.titleMedium,
        modifier = Modifier.padding(top = 24.dp),
    )
    for (state in backup.insteadOf) {
        val (words, color) = lineState(state.severity)
        // Read as one: "Metro 12, interrupted".
        Row(Modifier.padding(top = 8.dp).semantics(mergeDescendants = true) {}, verticalAlignment = Alignment.CenterVertically) {
            LineBadge(state.line, state.color)
            Spacer(Modifier.width(12.dp))
            Text(words, color = color)
        }
    }
    for (leg in backup.legs) LegDepartures(leg, now)
}

/** A leg in its card. [onPick], on the leg the reminder is for, with why the last pick was [refused]. */
@Composable
private fun LegDepartures(leg: Departures.Leg, now: Instant, refused: Int? = null, onPick: ((Departure) -> Unit)? = null) {
    Card(
        Modifier.fillMaxWidth().padding(top = 12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
            // Read as one: "Line J, Les Aubépines to Gare Centrale", and how a backup's ride runs.
            Column(Modifier.semantics(mergeDescendants = true) {}) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    LineBadge(leg.line, leg.color)
                    if (leg.from != null && leg.to != null) {
                        Spacer(Modifier.width(12.dp))
                        StopName(leg.from, Modifier.weight(1f, fill = false))
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowForward,
                            contentDescription = stringResource(R.string.departures_leg_to),
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(horizontal = 6.dp).size(16.dp),
                        )
                        StopName(leg.to, Modifier.weight(1f, fill = false))
                    }
                }
                leg.severity?.let { severity ->
                    val (words, color) = lineState(severity)
                    Row(Modifier.padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Filled.Warning, contentDescription = null, tint = color, modifier = Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text(words, color = color)
                    }
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
                // The metro gives minutes only: its next trains fit on a line.
                leg.reach == null && departures.all { it.aimed == null } -> MetroTimes(departures, now)
                else -> {
                    var more by rememberSaveable(leg.line.name, leg.from, leg.to) { mutableStateOf(false) }
                    val short = leg.shortList(now)
                    (if (more) departures else short).forEachIndexed { index, departure ->
                        if (index > 0) HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                        DepartureRow(leg, departure, now, onPick)
                    }
                    val hidden = departures.size - short.size
                    if (hidden > 0) {
                        TextButton(onClick = { more = !more }) {
                            Text(if (more) stringResource(R.string.departures_less) else pluralStringResource(R.plurals.departures_more, hidden, hidden))
                        }
                    }
                }
            }
        }
    }
}

/**
 * A metro's next trains, one line per destination, read as one:
 * "Arriving · 6 · 9 · 13 min, Mairie d'Issy".
 */
@Composable
private fun MetroTimes(departures: List<Departure>, now: Instant) {
    for ((destination, trains) in departures.groupBy { it.destination }) {
        Row(
            Modifier.fillMaxWidth().padding(top = 8.dp).semantics(mergeDescendants = true) {},
            verticalAlignment = Alignment.CenterVertically,
        ) {
            trains.forEachIndexed { index, train ->
                if (index > 0) Text(" · ", color = MaterialTheme.colorScheme.onSurfaceVariant)
                val minutes = train.expected.minutesFrom(now)
                Text(
                    when {
                        minutes == 0L -> stringResource(R.string.departures_arriving)
                        index == trains.lastIndex -> stringResource(R.string.departures_minutes, minutes)
                        else -> minutes.toString()
                    },
                    style = MaterialTheme.typography.titleMedium.tabular(),
                    fontWeight = FontWeight.Bold,
                )
            }
            Spacer(Modifier.width(12.dp))
            Text(destination, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

/**
 * One departure of [leg]: when it leaves and arrives, where it goes, then its
 * platform, delay and when to leave small below, read as one: "07:07, arrives
 * 07:23, Gare Centrale, Platform 2, +4 min, leave in 9 min".
 * With [onPick], a train that can still be caught has a bell that picks it for
 * the reminder to leave, and drops it once picked.
 */
@Composable
private fun DepartureRow(leg: Departures.Leg, departure: Departure, now: Instant, onPick: ((Departure) -> Unit)?) {
    val pickable = onPick != null && departure.journey != null && (departure.picked || leg.catchable(departure, now))
    val cancelled = departure.state == State.CANCELLED
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).semantics(mergeDescendants = true) {}) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                // Figures of one width: the destinations line up from row to row.
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        departure.knownBy.clock(),
                        style = MaterialTheme.typography.titleMedium.tabular(),
                        fontWeight = FontWeight.Bold,
                        maxLines = 1,
                        textDecoration = if (cancelled) TextDecoration.LineThrough else null,
                    )
                    departure.arrival?.let { arrival ->
                        val arrives = stringResource(R.string.departures_arrival, arrival.clock())
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowForward,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(horizontal = 4.dp).size(14.dp),
                        )
                        Text(
                            arrival.clock(),
                            style = MaterialTheme.typography.titleSmall.tabular(),
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            modifier = Modifier.semantics { contentDescription = arrives },
                        )
                    }
                }
                Spacer(Modifier.width(12.dp))
                Text(
                    departure.destination,
                    style = MaterialTheme.typography.bodyLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
            }
            Row(Modifier.padding(top = 2.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                val details = MaterialTheme.typography.bodySmall
                // Below the time it leaves at: where it leaves from.
                departure.platform?.let { Tag(stringResource(R.string.departures_platform, it)) }
                when {
                    cancelled -> Text(stringResource(R.string.departures_cancelled), style = details, color = MaterialTheme.colorScheme.error)
                    departure.delayMinutes > 0 -> Text(
                        stringResource(R.string.departures_late, departure.delayMinutes),
                        style = details,
                        color = MaterialTheme.colorScheme.primary,
                        fontWeight = FontWeight.Bold,
                    )
                    departure.state == State.DELAYED -> Text(stringResource(R.string.departures_delayed), style = details, color = MaterialTheme.colorScheme.primary)
                }
                if (departure.picked) {
                    Text(stringResource(R.string.departures_picked), style = details, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
                }
                if (departure.usual) Text(stringResource(R.string.departures_usual), style = details, color = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.weight(1f))
                Countdown(leg, departure, now)
            }
        }
        when {
            pickable -> Bell(departure, onPick)
            // The bell's room, so that the times to leave line up.
            onPick != null -> Spacer(Modifier.width(48.dp))
        }
    }
}

/** When to leave for [departure], or, too soon for that, when it leaves; nothing for a cancelled one. */
@Composable
private fun Countdown(leg: Departures.Leg, departure: Departure, now: Instant) {
    val leave = leg.leaveAt(departure) ?: return
    if (departure.state == State.CANCELLED) return
    val left = Duration.between(now, leave)
    val style = MaterialTheme.typography.bodyMedium
    when {
        // Too soon to leave for, but not for whoever is already on the way.
        left.isNegative -> Text(leavesIn(departure, now), style = style, color = MaterialTheme.colorScheme.onSurfaceVariant)
        left.toMinutes() == 0L -> Text(stringResource(R.string.departures_leave_now), style = style, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
        else -> Text(stringResource(R.string.departures_leave_in, left.toMinutes()), style = style, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
    }
}

/** A stop's name on one line: a long one ends with "…". */
@Composable
private fun StopName(name: String, modifier: Modifier) {
    Text(name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = modifier)
}

/** A small label on its own ground, such as the platform. */
@Composable
internal fun Tag(text: String) {
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
