package io.github.nonock.eztravel.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Alerts
import io.github.nonock.eztravel.backend.Alerts.Alert
import io.github.nonock.eztravel.backend.Alerts.Severity
import io.github.nonock.eztravel.backend.Backend
import io.github.nonock.eztravel.notifications.label
import kotlinx.coroutines.delay
import org.json.JSONException
import java.io.IOException
import java.time.Instant
import java.time.ZoneId

/** The backend reads the disruptions at most every five minutes, or every two while watching. */
private const val REFRESH_MS = 120_000L

/** Lines folded to their first lines of text, until the card is tapped. */
private const val FOLDED_LINES = 3

private sealed interface Read {
    data object Loading : Read
    data object Unreadable : Read
    data class Known(val lines: List<Alerts.LineAlerts>) : Read
}

/**
 * Every disruption of the lines that is not over yet, line by line: under way
 * first, the worst first, then those to come. Read when shown, and every two
 * minutes while it is.
 */
@Composable
fun AlertsPage(backend: Backend, modifier: Modifier = Modifier, now: () -> Instant = Instant::now) {
    var read by remember { mutableStateOf<Read>(Read.Loading) }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(backend) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            while (true) {
                read = try {
                    // Unreadable is not "no alert": the backend says when it could not read them.
                    backend.alerts().lines?.let { Read.Known(it) } ?: Read.Unreadable
                } catch (_: IOException) {
                    Read.Unreadable
                } catch (_: JSONException) {
                    Read.Unreadable
                }
                delay(REFRESH_MS)
            }
        }
    }

    Column(modifier) {
        when (val current = read) {
            Read.Loading -> Text(stringResource(R.string.away_loading))
            Read.Unreadable -> Text(stringResource(R.string.alerts_unavailable), color = MaterialTheme.colorScheme.error)
            is Read.Known -> {
                if (current.lines.all { it.alerts.isEmpty() }) {
                    Text(stringResource(R.string.alerts_none_at_all), style = MaterialTheme.typography.titleMedium)
                }
                for (line in current.lines) LineSection(line, now())
            }
        }
    }
}

@Composable
private fun LineSection(line: Alerts.LineAlerts, now: Instant) {
    // Read once, from the badge: "Line J".
    Row(
        Modifier.padding(top = 16.dp, bottom = 4.dp).semantics(mergeDescendants = true) {},
        verticalAlignment = Alignment.CenterVertically,
    ) {
        LineBadge(line.line, line.color)
        Spacer(Modifier.width(12.dp))
        Text(line.line.label(LocalResources.current), style = MaterialTheme.typography.titleMedium, modifier = Modifier.clearAndSetSemantics {})
    }
    if (line.alerts.isEmpty()) {
        Text(stringResource(R.string.alerts_none), color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
    for ((alert, more) in line.grouped) AlertCard(alert, more, now)
}

/** One alert, folded to its first lines until tapped; [more] alerts share its title. */
@Composable
private fun AlertCard(alert: Alert, more: Int, now: Instant) {
    var unfolded by rememberSaveable(alert.id) { mutableStateOf(false) }
    val today = now.atZone(ZoneId.systemDefault()).toLocalDate()
    Card(
        Modifier
            .fillMaxWidth()
            .padding(top = 8.dp)
            .clickable(onClickLabel = stringResource(if (unfolded) R.string.alerts_fold else R.string.alerts_unfold)) { unfolded = !unfolded },
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Row(Modifier.padding(16.dp)) {
            val (icon, tint) = when (alert.severity) {
                Severity.BLOCKING -> Icons.Filled.Warning to MaterialTheme.colorScheme.error
                Severity.DISRUPTED -> Icons.Filled.Warning to MaterialTheme.colorScheme.primary
                Severity.INFORMATION -> Icons.Outlined.Info to MaterialTheme.colorScheme.onSurfaceVariant
            }
            Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(24.dp))
            Spacer(Modifier.width(12.dp))
            Column {
                Text(alert.title, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.Bold)
                Row(Modifier.padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Tag(stringResource(if (alert.active) R.string.alerts_now else R.string.alerts_coming))
                    if (alert.onWay) Tag(stringResource(R.string.alerts_on_way))
                }
                Text(
                    if (alert.active) {
                        stringResource(R.string.alerts_until, alert.end.shown(today))
                    } else {
                        stringResource(R.string.alerts_from, alert.begin.shown(today))
                    },
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 6.dp),
                )
                if (more > 0) {
                    Text(
                        pluralStringResource(R.plurals.alerts_more_periods, more, more),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Text(
                    alert.text,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = if (unfolded) Int.MAX_VALUE else FOLDED_LINES,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
        }
    }
}
