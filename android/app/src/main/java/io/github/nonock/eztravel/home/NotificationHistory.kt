package io.github.nonock.eztravel.home

import android.content.res.Resources
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material.icons.outlined.DateRange
import androidx.compose.material3.Icon
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Backend
import io.github.nonock.eztravel.backend.HistoryEntry
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import io.github.nonock.eztravel.notifications.label
import io.github.nonock.eztravel.notifications.text
import io.github.nonock.eztravel.ui.theme.Good
import org.json.JSONException
import java.io.IOException
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

private sealed interface Sent {
    data object Loading : Sent
    data object Unreachable : Sent
    data class Known(val entries: List<HistoryEntry>) : Sent
}

/** What the backend sent, by day, newest first. */
@Composable
fun NotificationHistory(backend: Backend, modifier: Modifier = Modifier, refresh: Int = 0, onRead: () -> Unit = {}) {
    var sent by remember { mutableStateOf<Sent>(Sent.Loading) }
    LaunchedEffect(backend, refresh) {
        sent = try {
            Sent.Known(backend.history())
        } catch (_: IOException) {
            Sent.Unreachable
        } catch (_: JSONException) {
            Sent.Unreachable
        }
        onRead()
    }

    Column(modifier) {
        when (val current = sent) {
            Sent.Loading -> Text(stringResource(R.string.away_loading))
            Sent.Unreachable -> Text(stringResource(R.string.away_unreachable), color = MaterialTheme.colorScheme.error)
            is Sent.Known -> if (current.entries.isEmpty()) {
                Text(stringResource(R.string.history_empty))
            } else {
                Days(current.entries)
            }
        }
    }
}

@Composable
private fun Days(entries: List<HistoryEntry>) {
    val zone = ZoneId.systemDefault()
    val today = LocalDate.now(zone)
    for ((day, ofDay) in entries.groupBy { it.at.atZone(zone).toLocalDate() }) {
        Text(
            when (day) {
                today -> stringResource(R.string.history_today)
                today.minusDays(1) -> stringResource(R.string.history_yesterday)
                else -> DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL).format(day)
            },
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.padding(top = 16.dp, bottom = 4.dp),
        )
        for (entry in ofDay) Entry(entry, zone)
    }
}

@Composable
private fun Entry(entry: HistoryEntry, zone: ZoneId) {
    val resources = LocalResources.current
    val time = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).format(entry.at.atZone(zone))
    val look = entry.look()
    // One entry, read as one: its mark, its time, its words. The rail tells how it went.
    Row(
        Modifier
            .fillMaxWidth()
            .padding(vertical = 6.dp)
            .height(IntrinsicSize.Min)
            .semantics(mergeDescendants = true) {},
    ) {
        Box(Modifier.width(3.dp).fillMaxHeight().background(look.tone.color(), RoundedCornerShape(2.dp)))
        Spacer(Modifier.width(12.dp))
        Box(Modifier.size(32.dp), contentAlignment = Alignment.Center) { EntryMark(look.mark) }
        Spacer(Modifier.width(12.dp))
        EntryWords(entry, time, resources)
    }
}

@Composable
private fun Tone.color(): Color = when (this) {
    Tone.BAD -> MaterialTheme.colorScheme.error
    Tone.WARN -> MaterialTheme.colorScheme.primary
    Tone.GOOD -> Good
    Tone.INFO -> MaterialTheme.colorScheme.outline
}

@Composable
private fun EntryMark(mark: Mark) {
    when (mark) {
        is Mark.Line -> LineBadge(mark.line, color = null)
        Mark.Reminder -> Icon(Icons.Filled.Notifications, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
        Mark.WatchDown -> Icon(Icons.Filled.Warning, contentDescription = null, tint = MaterialTheme.colorScheme.error)
        Mark.WatchUp -> Icon(Icons.Filled.CheckCircle, contentDescription = null, tint = Good)
        Mark.Planned -> Icon(Icons.Outlined.DateRange, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun EntryWords(entry: HistoryEntry, time: String, resources: Resources) {
    Column {
        Text(time, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        when (entry) {
            is HistoryEntry.Sent -> {
                val shown = entry.message.text(resources)
                Text(shown.title, style = MaterialTheme.typography.bodyLarge)
                if (shown.body.isNotEmpty()) {
                    Text(
                        shown.body,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 4,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            is HistoryEntry.WindowClosed -> Text(
                stringResource(
                    if (entry.severity == Severity.BLOCKING) R.string.history_closed_blocking else R.string.history_closed_disrupted,
                    entry.line.label(resources),
                ),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
