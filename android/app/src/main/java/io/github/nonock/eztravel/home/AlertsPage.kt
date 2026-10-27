package io.github.nonock.eztravel.home

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.clickable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Alerts
import io.github.nonock.eztravel.backend.Alerts.Alert
import io.github.nonock.eztravel.backend.Alerts.Severity
import io.github.nonock.eztravel.backend.Backend
import io.github.nonock.eztravel.notifications.Opening
import io.github.nonock.eztravel.notifications.label
import java.time.Instant
import java.time.ZoneId

/** The backend reads the disruptions at most every five minutes, or every two while watching. */
private const val REFRESH_MS = 120_000L

/** Lines folded to their first lines of text, until the card is tapped. */
private const val FOLDED_LINES = 3

/**
 * Every disruption of the lines that is not over yet, line by line: under way
 * first, the worst first, then those to come. Read when shown, every two
 * minutes while it is, and when [refresh] changes; [onRead] follows each read.
 * Opened from a notification, the [opening]'s line shows unfolded, and its
 * record in full.
 */
@Composable
fun AlertsPage(
    backend: Backend,
    modifier: Modifier = Modifier,
    now: () -> Instant = Instant::now,
    refresh: Int = 0,
    onRead: () -> Unit = {},
    opening: Opening? = null,
) {
    // Unreadable is not "no alert": the backend says when it could not read them.
    val read by polled(backend, refresh, everyMs = REFRESH_MS, onRead = onRead) { backend.alerts().lines }

    Column(modifier) {
        when (val current = read) {
            Reading.Loading -> Text(stringResource(R.string.loading))
            Reading.Failed -> Text(stringResource(R.string.alerts_unavailable), color = MaterialTheme.colorScheme.error)
            is Reading.Known -> {
                if (current.value.all { it.alerts.isEmpty() }) {
                    Text(stringResource(R.string.alerts_none_at_all), style = MaterialTheme.typography.titleMedium)
                }
                for (line in current.value) LineSection(line, now(), opening?.takeIf { it.line == line.line.name })
            }
        }
    }
}

/**
 * A line, folded to its header until tapped: what is under way and to come,
 * and the worst under way, so that nothing urgent hides in a fold. Unfolded
 * when [opened], with its record in full.
 */
@Composable
private fun LineSection(line: Alerts.LineAlerts, now: Instant, opened: Opening?) {
    var open by rememberSaveable(line.line.name) { mutableStateOf(opened != null) }
    val unfolded = opened?.let { line.unfolded(it.record, it.title) }
    val any = line.alerts.isNotEmpty()
    val tap = if (any) {
        Modifier.clickable(onClickLabel = stringResource(if (open) R.string.alerts_hide_line else R.string.alerts_show_line)) { open = !open }
    } else {
        Modifier
    }
    // Read as one, the line from its badge: "Line J, 1 now, 3 coming".
    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = 12.dp)
            .clip(RoundedCornerShape(12.dp))
            .then(tap)
            .semantics(mergeDescendants = true) {}
            .padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        LineBadge(line.line, line.color)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(line.line.label(LocalResources.current), style = MaterialTheme.typography.titleMedium, modifier = Modifier.clearAndSetSemantics {})
            val parts = listOfNotNull(
                line.worstUnderWay?.let { severityWords(it) },
                line.underWay.takeIf { it > 0 }?.let { pluralStringResource(R.plurals.alerts_under_way, it, it) },
                line.toCome.takeIf { it > 0 }?.let { pluralStringResource(R.plurals.alerts_to_come, it, it) },
            )
            Text(
                if (any) parts.joinToString(", ") else stringResource(R.string.alerts_none),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        line.worstUnderWay?.let { worst ->
            val (icon, tint) = severityMark(worst)
            Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(8.dp))
        }
        if (any) {
            Icon(
                if (open) Icons.Filled.KeyboardArrowUp else Icons.Filled.KeyboardArrowDown,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
    AnimatedVisibility(open) {
        Column {
            for ((alert, more) in line.grouped) AlertCard(alert, more, now, opened = alert.title == unfolded)
        }
    }
}

/** A severity's icon and color, the same on the header and on the cards. */
@Composable
private fun severityMark(severity: Severity): Pair<ImageVector, Color> = when (severity) {
    Severity.BLOCKING -> Icons.Filled.Warning to MaterialTheme.colorScheme.error
    Severity.DISRUPTED -> Icons.Filled.Warning to MaterialTheme.colorScheme.primary
    Severity.INFORMATION -> Icons.Outlined.Info to MaterialTheme.colorScheme.onSurfaceVariant
}

/** How bad, in words: blocking and disrupted share an icon, apart by color only; information has its own. */
@Composable
private fun severityWords(severity: Severity): String? = when (severity) {
    Severity.BLOCKING -> stringResource(R.string.status_state_blocking)
    Severity.DISRUPTED -> stringResource(R.string.status_state_disrupted)
    Severity.INFORMATION -> null
}

/** The alert's text with its web links, underlined, which open in the browser. */
@Composable
private fun linked(alert: Alert): AnnotatedString {
    val style = TextLinkStyles(SpanStyle(color = MaterialTheme.colorScheme.primary, textDecoration = TextDecoration.Underline))
    return buildAnnotatedString {
        append(alert.text)
        for (link in alert.links) addLink(LinkAnnotation.Url(link.url, style), link.start, link.end)
    }
}

/**
 * One alert, folded to its first lines until tapped, or [opened] in full and
 * in view; [more] alerts share its title. Its links work once unfolded, so
 * that a tap to unfold never opens the browser.
 */
@Composable
private fun AlertCard(alert: Alert, more: Int, now: Instant, opened: Boolean) {
    var unfolded by rememberSaveable(alert.id) { mutableStateOf(opened) }
    val today = now.atZone(ZoneId.systemDefault()).toLocalDate()
    val view = remember { BringIntoViewRequester() }
    if (opened) {
        // Once laid out, as the line unfolds.
        LaunchedEffect(Unit) {
            withFrameNanos {}
            view.bringIntoView()
        }
    }
    Card(
        Modifier
            .bringIntoViewRequester(view)
            .fillMaxWidth()
            .padding(top = 8.dp)
            .clickable(onClickLabel = stringResource(if (unfolded) R.string.alerts_fold else R.string.alerts_unfold)) { unfolded = !unfolded },
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Row(Modifier.padding(16.dp)) {
            val (icon, tint) = severityMark(alert.severity)
            Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(24.dp))
            Spacer(Modifier.width(12.dp))
            Column {
                Text(alert.title, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.Bold)
                // Three tags may not fit on one line, in large type or in French.
                FlowRow(
                    Modifier.padding(top = 6.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    Tag(stringResource(if (alert.active) R.string.alerts_now else R.string.alerts_coming))
                    severityWords(alert.severity)?.let { Tag(it) }
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
                    if (unfolded) linked(alert) else AnnotatedString(alert.text),
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = if (unfolded) Int.MAX_VALUE else FOLDED_LINES,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
        }
    }
}
