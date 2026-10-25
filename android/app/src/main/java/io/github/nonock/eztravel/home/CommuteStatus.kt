package io.github.nonock.eztravel.home

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material.icons.outlined.DateRange
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Backend
import io.github.nonock.eztravel.backend.Status
import io.github.nonock.eztravel.backend.Status.Cause
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.backend.Status.Headline
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import io.github.nonock.eztravel.notifications.label
import io.github.nonock.eztravel.ui.theme.Good
import java.time.Instant

/** Checks run every two minutes: reading every minute keeps the screen current. */
private const val REFRESH_MS = 60_000L

/**
 * What the backend sees, in a card headed by the one thing to know: all fine,
 * a line in trouble, or that nothing can be vouched for. Read when the screen
 * shows, every minute while it does, and when `refresh` changes.
 */
@Composable
fun CommuteStatus(backend: Backend, refresh: Int, modifier: Modifier = Modifier, now: () -> Instant = Instant::now) {
    // The commute tab already says that this build has no server.
    if (!backend.configured) return
    val seen by polled(backend, refresh, everyMs = REFRESH_MS) { backend.status() }

    Card(
        modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Column(Modifier.padding(16.dp)) {
            Text(
                stringResource(R.string.status_title),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            when (val current = seen) {
                Reading.Loading -> Text(stringResource(R.string.loading))
                Reading.Failed -> {
                    HeadlineRow(Icons.Filled.Warning, MaterialTheme.colorScheme.error, stringResource(R.string.status_headline_unreachable))
                    Text(stringResource(R.string.status_unreachable), color = MaterialTheme.colorScheme.error)
                }
                is Reading.Known -> Known(current.value, now())
            }
        }
    }
}

/** An icon and the headline, large: what to know at a glance. */
@Composable
private fun HeadlineRow(icon: ImageVector, tint: Color, text: String) {
    Row(Modifier.padding(bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(28.dp))
        Spacer(Modifier.width(12.dp))
        Text(text, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun Known(status: Status, now: Instant) {
    val resources = LocalResources.current
    when (val headline = status.headline(now)) {
        is Headline.Out -> HeadlineRow(Icons.Filled.Warning, MaterialTheme.colorScheme.error, stringResource(R.string.status_headline_outage))
        is Headline.Trouble -> {
            val blocking = headline.line.severity == Severity.BLOCKING
            HeadlineRow(
                Icons.Filled.Warning,
                if (blocking) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary,
                stringResource(
                    if (blocking) R.string.status_headline_blocking else R.string.status_headline_disrupted,
                    headline.line.line.label(resources),
                ),
            )
        }
        Headline.Fine -> HeadlineRow(Icons.Filled.CheckCircle, Good, stringResource(R.string.status_fine))
        Headline.Checking -> HeadlineRow(Icons.Filled.Info, MaterialTheme.colorScheme.onSurfaceVariant, stringResource(R.string.status_checking))
        Headline.Idle -> HeadlineRow(Icons.Outlined.DateRange, MaterialTheme.colorScheme.onSurfaceVariant, stringResource(R.string.status_not_watching))
    }
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
        null -> Text(status.next?.let { stringResource(R.string.status_next, it.shown()) } ?: stringResource(R.string.status_no_next))
        else -> {
            Text(
                stringResource(
                    if (status.watching == Direction.OUTBOUND) R.string.status_watching_outbound else R.string.status_watching_inbound,
                ),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            for (state in status.lines) {
                val (words, color) = lineState(state.severity)
                // One row, read as one: "Line J, interrupted".
                Row(
                    Modifier.padding(vertical = 4.dp).semantics(mergeDescendants = true) {},
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    LineBadge(state.line, state.color)
                    Spacer(Modifier.width(12.dp))
                    Text(words, color = color)
                }
            }
        }
    }
    status.checkedAt?.let {
        Text(stringResource(R.string.status_checked, it.shown()), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
    if (status.planned.isNotEmpty()) {
        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Outlined.DateRange, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text(stringResource(R.string.status_planned), fontWeight = FontWeight.Bold)
        }
        for (title in status.planned) Text(title)
    }
    status.schoolHoliday?.let { name ->
        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Filled.Info, contentDescription = null, tint = MaterialTheme.colorScheme.tertiary, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text(stringResource(R.string.status_school_holiday, name))
        }
    }
}
