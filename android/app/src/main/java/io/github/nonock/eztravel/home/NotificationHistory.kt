package io.github.nonock.eztravel.home

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
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
fun NotificationHistory(backend: Backend, onBack: () -> Unit, modifier: Modifier = Modifier) {
    var sent by remember { mutableStateOf<Sent>(Sent.Loading) }
    LaunchedEffect(backend) {
        sent = try {
            Sent.Known(backend.history())
        } catch (_: IOException) {
            Sent.Unreachable
        } catch (_: JSONException) {
            Sent.Unreachable
        }
    }

    Column(modifier) {
        TextButton(onClick = onBack) { Text(stringResource(R.string.history_back)) }
        Text(stringResource(R.string.history_title), style = MaterialTheme.typography.titleLarge)
        Spacer(Modifier.height(8.dp))
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
    Column(Modifier.padding(vertical = 6.dp)) {
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
