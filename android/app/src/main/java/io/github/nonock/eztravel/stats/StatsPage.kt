package io.github.nonock.eztravel.stats

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalLocale
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Backend
import io.github.nonock.eztravel.backend.Stats
import io.github.nonock.eztravel.backend.Status
import io.github.nonock.eztravel.home.LineBadge
import io.github.nonock.eztravel.home.shortDate
import io.github.nonock.eztravel.ui.theme.Good
import org.json.JSONException
import java.io.IOException
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.format.TextStyle

private sealed interface Reading {
    data object Loading : Reading

    data object Unreachable : Reading

    data class Known(val stats: Stats) : Reading
}

/**
 * How the trains ran, office days only: for each way, the share on time, in
 * words and as a bar, then by departure time and by weekday, and day by day
 * how its usual train ran. Last, the days each line was told disrupted.
 */
@Composable
fun StatsPage(backend: Backend, modifier: Modifier = Modifier, refresh: Int = 0, onRead: () -> Unit = {}) {
    var reading by remember { mutableStateOf<Reading>(Reading.Loading) }
    LaunchedEffect(backend, refresh) {
        reading = try {
            Reading.Known(backend.stats())
        } catch (_: IOException) {
            Reading.Unreachable
        } catch (_: JSONException) {
            Reading.Unreachable
        }
        onRead()
    }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        val current = reading
        when {
            current is Reading.Known && current.stats.ways.isEmpty() -> Text(stringResource(R.string.stats_empty))
            current is Reading.Known -> {
                current.stats.since?.let {
                    Text(
                        stringResource(R.string.stats_since, it.shortDate()),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                for (way in current.stats.ways) WayCard(way)
                current.stats.disruptions?.let { DisruptionsCard(it) }
            }
            current == Reading.Unreachable -> Text(stringResource(R.string.unreachable), color = MaterialTheme.colorScheme.error)
            else -> Text(stringResource(R.string.loading))
        }
    }
}

@Composable
private fun WayCard(way: Stats.Way) {
    val resources = LocalResources.current
    val count = way.count
    Card(Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                LineBadge(way.line, way.color)
                Spacer(Modifier.width(12.dp))
                Text(
                    stringResource(if (way.direction == Status.Direction.OUTBOUND) R.string.usual_outbound else R.string.usual_inbound),
                    style = MaterialTheme.typography.titleMedium,
                )
            }
            Text(
                stringResource(R.string.stats_on_time_share, count.onTimePercent),
                style = MaterialTheme.typography.headlineMedium,
                modifier = Modifier.padding(top = 12.dp),
            )
            Shares(count)
            Text(
                stringResource(
                    R.string.stats_count,
                    resources.getQuantityString(R.plurals.stats_trains, count.trains, count.trains),
                    count.onTime,
                    count.late,
                    resources.getQuantityString(R.plurals.stats_cancelled, count.cancelled, count.cancelled),
                ),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 8.dp),
            )
            if (way.times.isNotEmpty()) {
                Heading(R.string.stats_times)
                val format = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)
                for (slot in way.times) ShareRow(format.format(slot.time), slot.count, TIME_WIDTH, marked = slot.time == way.usual)
            }
            if (way.weekdays.isNotEmpty()) {
                Heading(R.string.stats_weekdays)
                val locale = LocalLocale.current.platformLocale
                for (weekday in way.weekdays) {
                    val name = weekday.day.getDisplayName(TextStyle.FULL, locale).let { it.substring(0, 1).uppercase(locale) + it.substring(1) }
                    val days = resources.getQuantityString(R.plurals.stats_days, weekday.days, weekday.days)
                    ShareRow(stringResource(R.string.stats_weekday, name, days), weekday.count, WEEKDAY_WIDTH)
                }
            }
            way.usual?.let { usual ->
                Heading(R.string.stats_usual, DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).format(usual))
                val tones = mapOf(
                    Tone.NONE to MaterialTheme.colorScheme.onSurfaceVariant,
                    Tone.GOOD to Good,
                    Tone.LATE to MaterialTheme.colorScheme.primary,
                    Tone.BAD to MaterialTheme.colorScheme.error,
                )
                for (day in way.days) {
                    val (words, tone) = ran(day.usual)
                    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                        Text(day.day.shortDate(), Modifier.weight(1f))
                        Text(stringResource(words, day.usual?.late ?: 0), color = tones.getValue(tone))
                    }
                }
            }
        }
    }
}

/** A part of a way's card. */
@Composable
private fun Heading(title: Int, vararg args: Any) {
    Text(stringResource(title, *args), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 16.dp, bottom = 4.dp))
}

private val TIME_WIDTH = 56.dp
private val WEEKDAY_WIDTH = 136.dp

/** The trains of a time or a day, [label]led: their shares as a bar, then how many of them on time; the usual train's [marked]. */
@Composable
private fun ShareRow(label: String, count: Stats.Count, labelWidth: Dp, marked: Boolean = false) {
    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(
            label,
            Modifier.width(labelWidth),
            color = if (marked) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
            fontWeight = if (marked) FontWeight.Bold else FontWeight.Normal,
        )
        Shares(count, Modifier.weight(1f).padding(horizontal = 12.dp), height = 6.dp)
        Text(
            stringResource(R.string.stats_on_time_of, count.onTime, count.trains),
            Modifier.widthIn(min = 48.dp),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.End,
        )
    }
}

/** For each line told disrupted, the days with an alert and the alerts; since when the history keeps them. */
@Composable
private fun DisruptionsCard(disruptions: Stats.Disruptions) {
    val resources = LocalResources.current
    Card(Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.stats_disruptions), style = MaterialTheme.typography.titleMedium)
            disruptions.since?.let {
                Text(stringResource(R.string.stats_since, it.shortDate()), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            if (disruptions.lines.isEmpty()) Text(stringResource(R.string.stats_disruptions_none))
            for (line in disruptions.lines) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    LineBadge(line.line, line.color)
                    Spacer(Modifier.width(12.dp))
                    Text(
                        stringResource(
                            R.string.stats_disrupted,
                            resources.getQuantityString(R.plurals.stats_days_disrupted, line.days, line.days),
                            resources.getQuantityString(R.plurals.stats_alerts, line.alerts, line.alerts),
                        ),
                    )
                }
            }
        }
    }
}

/** On time, late and cancelled side by side, each as wide as its share: the words beside say the same. */
@Composable
private fun Shares(count: Stats.Count, modifier: Modifier = Modifier.fillMaxWidth().padding(top = 8.dp), height: Dp = 8.dp) {
    val parts = listOf(count.onTime to Good, count.late to MaterialTheme.colorScheme.primary, count.cancelled to MaterialTheme.colorScheme.error)
    Row(modifier.height(height).clip(RoundedCornerShape(height / 2))) {
        for ((trains, color) in parts.filter { it.first > 0 }) {
            Box(Modifier.weight(trains.toFloat()).fillMaxHeight().background(color))
        }
    }
}

private enum class Tone { NONE, GOOD, LATE, BAD }

/** How the usual train ran: its words, given how late, and their tone; none that day. */
private fun ran(usual: Stats.UsualRun?): Pair<Int, Tone> = when (usual?.run) {
    null -> R.string.stats_run_none to Tone.NONE
    Stats.Run.ON_TIME -> R.string.stats_run_on_time to Tone.GOOD
    Stats.Run.LATE -> R.string.stats_run_late to Tone.LATE
    Stats.Run.CANCELLED -> R.string.stats_run_cancelled to Tone.BAD
}
