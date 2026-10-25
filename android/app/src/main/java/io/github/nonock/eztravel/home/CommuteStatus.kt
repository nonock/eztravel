package io.github.nonock.eztravel.home

import androidx.compose.foundation.clickable
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
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
import io.github.nonock.eztravel.backend.Status
import io.github.nonock.eztravel.backend.Status.Cause
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.backend.Status.Headline
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import io.github.nonock.eztravel.notifications.label
import io.github.nonock.eztravel.ui.theme.Good
import java.time.Instant

/** Checks run every two minutes: reading every minute keeps the screen current. */
const val STATUS_REFRESH_MS = 60_000L

/** The one thing to know of what the backend sees, as an icon in its color and in words. */
private class Headed(val icon: ImageVector, val tint: Color, val words: String, val wrong: Boolean)

/** All fine, a line in trouble, or that nothing can be vouched for; none while reading. */
@Composable
private fun look(reading: Reading<Status>, now: Instant): Headed? = when (reading) {
    Reading.Loading -> null
    Reading.Failed -> Headed(Icons.Filled.Warning, MaterialTheme.colorScheme.error, stringResource(R.string.status_headline_unreachable), wrong = true)
    is Reading.Known -> when (val headline = reading.value.headline(now)) {
        is Headline.Out -> Headed(Icons.Filled.Warning, MaterialTheme.colorScheme.error, stringResource(R.string.status_headline_outage), wrong = true)
        is Headline.Trouble -> {
            val blocking = headline.line.severity == Severity.BLOCKING
            Headed(
                Icons.Filled.Warning,
                if (blocking) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary,
                stringResource(
                    if (blocking) R.string.status_headline_blocking else R.string.status_headline_disrupted,
                    headline.line.line.label(LocalResources.current),
                ),
                wrong = true,
            )
        }
        Headline.Fine -> Headed(Icons.Filled.CheckCircle, Good, stringResource(R.string.status_fine), wrong = false)
        Headline.Checking -> Headed(Watching, MaterialTheme.colorScheme.onSurfaceVariant, stringResource(R.string.status_checking), wrong = false)
        Headline.Idle -> Headed(NotWatching, MaterialTheme.colorScheme.onSurfaceVariant, stringResource(R.string.status_not_watching), wrong = false)
    }
}

/** Beside the gear: how the watch goes, named by its headline; [onOpen] shows the details. */
@Composable
fun StatusIcon(reading: Reading<Status>, now: Instant, onOpen: () -> Unit) {
    val look = look(reading, now)
    IconButton(onClick = onOpen) {
        Icon(
            look?.icon ?: Icons.Filled.Info,
            contentDescription = look?.words ?: stringResource(R.string.loading),
            tint = look?.tint ?: MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/** On the commute, only when something is wrong, which an icon alone would let slip by; [onOpen] shows the details. */
@Composable
fun StatusBanner(reading: Reading<Status>, now: Instant, modifier: Modifier = Modifier, onOpen: () -> Unit) {
    val look = look(reading, now)?.takeIf { it.wrong } ?: return
    Card(
        modifier.fillMaxWidth().clickable(onClick = onOpen),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Row(Modifier.padding(horizontal = 16.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(look.icon, contentDescription = null, tint = look.tint, modifier = Modifier.size(24.dp))
            Spacer(Modifier.width(12.dp))
            Text(look.words, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        }
    }
}

/** On the commute during school holidays, when fewer trains run: which, and how long. */
@Composable
fun SchoolHoliday(reading: Reading<Status>, modifier: Modifier = Modifier) {
    val words = (reading as? Reading.Known)?.value?.let { holidayWords(it) } ?: return
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        Icon(Icons.Filled.Info, contentDescription = null, tint = MaterialTheme.colorScheme.tertiary, modifier = Modifier.size(16.dp))
        Spacer(Modifier.width(8.dp))
        Text(words, style = MaterialTheme.typography.bodySmall)
    }
}

/** "Vacances de la Toussaint, du 17 oct. au 1 nov.", or their name alone from an older backend. */
@Composable
private fun holidayWords(status: Status): String? {
    val name = status.schoolHoliday ?: return null
    val first = status.schoolHolidayFirst ?: return name
    val last = status.schoolHolidayLast ?: return name
    return stringResource(R.string.status_school_holiday, name, first.shortDate(), last.shortDate())
}

/**
 * All the backend sees, headed by the one thing to know: the way watched and
 * how each line runs, the last check, the next window, what is planned today.
 */
@Composable
fun StatusDetails(reading: Reading<Status>, now: Instant, modifier: Modifier = Modifier) {
    Column(modifier) {
        val look = look(reading, now)
        if (look == null) {
            Text(stringResource(R.string.loading))
            return@Column
        }
        Row(Modifier.padding(bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(look.icon, contentDescription = null, tint = look.tint, modifier = Modifier.size(28.dp))
            Spacer(Modifier.width(12.dp))
            Text(look.words, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
        }
        when (reading) {
            is Reading.Known -> Known(reading.value)
            else -> Text(stringResource(R.string.status_unreachable), color = MaterialTheme.colorScheme.error)
        }
    }
}

@Composable
private fun Known(status: Status) {
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
    holidayWords(status)?.let { words ->
        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Filled.Info, contentDescription = null, tint = MaterialTheme.colorScheme.tertiary, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text(words)
        }
    }
}
