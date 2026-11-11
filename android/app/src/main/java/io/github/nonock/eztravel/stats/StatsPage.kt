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
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
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

private sealed interface Reading {
    data object Loading : Reading

    data object Unreachable : Reading

    data class Known(val stats: Stats) : Reading
}

/**
 * How the trains ran, office days only: for each way, the share on time, in
 * words and as a bar, then, day by day, how its usual train ran.
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
            way.usual?.let { usual ->
                Text(
                    stringResource(R.string.stats_usual, DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).format(usual)),
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.padding(top = 16.dp, bottom = 4.dp),
                )
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

/** On time, late and cancelled side by side, each as wide as its share: the words above say the same. */
@Composable
private fun Shares(count: Stats.Count) {
    val parts = listOf(count.onTime to Good, count.late to MaterialTheme.colorScheme.primary, count.cancelled to MaterialTheme.colorScheme.error)
    Row(Modifier.fillMaxWidth().padding(top = 8.dp).height(8.dp).clip(RoundedCornerShape(4.dp))) {
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
