package io.github.nonock.eztravel.calendar

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalLocale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Backend
import io.github.nonock.eztravel.backend.CalendarDay
import io.github.nonock.eztravel.backend.CalendarDay.Kind
import io.github.nonock.eztravel.backend.CalendarDay.Why
import io.github.nonock.eztravel.backend.DayRefusal
import io.github.nonock.eztravel.settings.toUtcDate
import io.github.nonock.eztravel.settings.toUtcMillis
import io.github.nonock.eztravel.ui.theme.AwayDay
import io.github.nonock.eztravel.ui.theme.Navy
import io.github.nonock.eztravel.ui.theme.OfficeDay
import io.github.nonock.eztravel.ui.theme.RemoteDay
import kotlinx.coroutines.launch
import org.json.JSONException
import java.io.IOException
import java.time.DayOfWeek
import java.time.LocalDate
import java.time.YearMonth
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.format.TextStyle
import java.util.Locale

private sealed interface Shown {
    data object Loading : Shown
    data object Unreachable : Shown
    data class Known(val month: YearMonth, val days: List<CalendarDay>) : Shown
}

/** The months shown after this one: the backend looks a year ahead. */
private const val AHEAD = 11L

/** The last day that can be set, from today: the backend looks a year ahead. */
private const val LAST_DAY = 365L

private val DayShape = RoundedCornerShape(10.dp)

/** "November 2026", as a month heads the calendar. */
fun monthTitle(month: YearMonth, locale: Locale = Locale.getDefault()): String =
    DateTimeFormatter.ofPattern("LLLL yyyy", locale).format(month).replaceFirstChar { it.titlecase(locale) }

/**
 * A month of days, office, remote work and days off each in its own color, a
 * week a row from Monday, as the two weeks' cycle goes: a tap sets a single
 * day, from today on, or days off through a date. `onSaved` follows each
 * change saved; the month is read again when [refresh] changes, then
 * `onRead`. A build without a server shows nothing.
 */
@Composable
fun CalendarPage(
    backend: Backend,
    modifier: Modifier = Modifier,
    today: LocalDate = LocalDate.now(),
    refresh: Int = 0,
    onRead: () -> Unit = {},
    onSaved: () -> Unit = {},
) {
    if (!backend.configured) return
    val first = YearMonth.from(today)
    var month by rememberSaveable { mutableStateOf(first) }
    var shown by remember { mutableStateOf<Shown>(Shown.Loading) }
    var refused by remember { mutableStateOf<DayRefusal?>(null) }
    var saves by remember { mutableIntStateOf(0) }
    // The first day off, while its last is chosen.
    var awayFrom by remember { mutableStateOf<LocalDate?>(null) }
    val scope = rememberCoroutineScope()
    val locale = LocalLocale.current.platformLocale

    LaunchedEffect(backend, month, refresh, saves) {
        shown = reach { Shown.Known(month, backend.month(month)) }
        onRead()
    }

    fun set(first: LocalDate, last: LocalDate, kind: Kind?) {
        scope.launch {
            refused = null
            // Once saved, the month is read again.
            shown = reach {
                refused = backend.setDays(first, last, kind)
                if (refused == null) {
                    onSaved()
                    saves++
                }
                shown
            }
        }
    }

    Column(modifier) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = { month = month.minusMonths(1) }, enabled = month > first) {
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, contentDescription = stringResource(R.string.calendar_previous))
            }
            Text(
                monthTitle(month, locale),
                style = MaterialTheme.typography.titleMedium,
                textAlign = TextAlign.Center,
                modifier = Modifier.weight(1f),
            )
            IconButton(onClick = { month = month.plusMonths(1) }, enabled = month < first.plusMonths(AHEAD)) {
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = stringResource(R.string.calendar_next))
            }
        }
        Row(Modifier.padding(vertical = 4.dp)) {
            for (weekday in DayOfWeek.entries) {
                Text(
                    weekday.getDisplayName(TextStyle.NARROW, locale),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.weight(1f),
                )
            }
        }
        when (val current = shown) {
            Shown.Loading -> Text(stringResource(R.string.loading))
            Shown.Unreachable -> Text(stringResource(R.string.calendar_unreachable))
            // Another month's days are not shown under this one's title.
            is Shown.Known -> if (current.month == month) {
                Weeks(current.days, today, onSet = { date, kind -> set(date, date, kind) }, onAwayUntil = { awayFrom = it })
            } else {
                Text(stringResource(R.string.loading))
            }
        }
        refused?.let {
            Text(
                stringResource(
                    when (it) {
                        DayRefusal.PAST -> R.string.calendar_past
                        DayRefusal.TOO_FAR -> R.string.calendar_too_far
                    },
                ),
                color = MaterialTheme.colorScheme.error,
                modifier = Modifier.padding(top = 8.dp),
            )
        }
        awayFrom?.let { first ->
            LastDayOffPicker(
                first = first,
                last = today.plusDays(LAST_DAY),
                onPick = { last ->
                    awayFrom = null
                    set(first, last, Kind.AWAY)
                },
                onDismiss = { awayFrom = null },
            )
        }
        Legend()
        Text(
            stringResource(R.string.calendar_explained),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 12.dp),
        )
    }
}

/** A call to the backend: what it gives, or `Unreachable` when it fails. */
private suspend fun reach(call: suspend () -> Shown): Shown = try {
    call()
} catch (_: IOException) {
    Shown.Unreachable
} catch (_: JSONException) {
    Shown.Unreachable
}

/** The month's days, a week a row from Monday. */
@Composable
private fun Weeks(days: List<CalendarDay>, today: LocalDate, onSet: (LocalDate, Kind?) -> Unit, onAwayUntil: (LocalDate) -> Unit) {
    val before = days.firstOrNull()?.date?.dayOfWeek?.ordinal ?: 0
    val cells = List<CalendarDay?>(before) { null } + days
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        for (week in cells.chunked(DayOfWeek.entries.size)) {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                for (index in DayOfWeek.entries.indices) {
                    Box(
                        Modifier
                            .weight(1f)
                            .aspectRatio(1f),
                    ) {
                        week.getOrNull(index)?.let { Day(it, today, onSet, onAwayUntil) }
                    }
                }
            }
        }
    }
}

/** What a day is, as the legend and a screen reader name it. */
private fun CalendarDay.label() = when (kind) {
    Kind.OFFICE -> R.string.calendar_office
    Kind.REMOTE -> R.string.calendar_remote
    Kind.AWAY -> R.string.calendar_away
    Kind.OFF -> if (why == Why.HOLIDAY) R.string.calendar_holiday else R.string.calendar_weekend
}

private fun Kind.fill(): Color = when (this) {
    Kind.OFFICE -> OfficeDay
    Kind.REMOTE -> RemoteDay
    Kind.AWAY -> AwayDay
    Kind.OFF -> Color.Transparent
}

/** A day of the grid; from today on, a tap opens what it can be set to, or days off from it. */
@Composable
private fun Day(day: CalendarDay, today: LocalDate, onSet: (LocalDate, Kind?) -> Unit, onAwayUntil: (LocalDate) -> Unit) {
    var choosing by remember { mutableStateOf(false) }
    val past = day.date < today
    val colors = MaterialTheme.colorScheme
    val fill = if (past) Color.Transparent else day.kind.fill()
    val ink = when {
        past -> colors.onSurfaceVariant.copy(alpha = 0.5f)
        fill != Color.Transparent -> Navy
        else -> colors.onSurfaceVariant
    }
    val description = listOfNotNull(
        DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL).withLocale(LocalLocale.current.platformLocale).format(day.date),
        stringResource(day.label()),
        stringResource(R.string.calendar_set).takeIf { day.why == Why.SET },
        day.schoolHoliday,
        stringResource(R.string.calendar_today).takeIf { day.date == today },
    ).joinToString(", ")
    val ring = when {
        day.date == today -> Modifier.border(2.dp, colors.onSurface, DayShape)
        !past && day.why == Why.HOLIDAY -> Modifier.border(1.dp, colors.outline, DayShape)
        else -> Modifier
    }
    val change = stringResource(R.string.calendar_change)
    Box(
        Modifier
            .fillMaxSize()
            .clip(DayShape)
            .background(fill)
            .then(ring)
            .then(if (past) Modifier else Modifier.clickable(onClickLabel = change) { choosing = true })
            .semantics(mergeDescendants = true) { contentDescription = description },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            day.date.dayOfMonth.toString(),
            color = ink,
            style = MaterialTheme.typography.bodyLarge,
            fontWeight = if (day.date == today) FontWeight.Bold else FontWeight.Medium,
        )
        if (day.schoolHoliday != null) SchoolMark(ink, Modifier.align(Alignment.TopCenter).padding(top = 5.dp))
        if (day.why == Why.SET) {
            Box(
                Modifier
                    .align(Alignment.BottomCenter)
                    .padding(bottom = 5.dp)
                    .size(4.dp)
                    .clip(CircleShape)
                    .background(ink),
            )
        }
        DropdownMenu(expanded = choosing, onDismissRequest = { choosing = false }) {
            for ((kind, name) in listOf(Kind.OFFICE to R.string.calendar_office, Kind.REMOTE to R.string.calendar_remote, Kind.AWAY to R.string.calendar_away)) {
                val current = kind == day.kind
                DropdownMenuItem(
                    text = { Text(stringResource(name)) },
                    onClick = {
                        choosing = false
                        if (!current) onSet(day.date, kind)
                    },
                    leadingIcon = { if (current) Icon(Icons.Filled.Check, contentDescription = null) },
                    modifier = Modifier.semantics { selected = current },
                )
            }
            DropdownMenuItem(
                text = { Text(stringResource(R.string.calendar_away_until)) },
                onClick = {
                    choosing = false
                    onAwayUntil(day.date)
                },
            )
            if (day.why == Why.SET) {
                HorizontalDivider()
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.calendar_as_planned)) },
                    onClick = {
                        choosing = false
                        onSet(day.date, null)
                    },
                )
            }
        }
    }
}

/** The last day off from [first], up to [last]: [first] itself unless another is chosen. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun LastDayOffPicker(first: LocalDate, last: LocalDate, onPick: (LocalDate) -> Unit, onDismiss: () -> Unit) {
    val state = rememberDatePickerState(
        initialSelectedDateMillis = first.toUtcMillis(),
        selectableDates = object : SelectableDates {
            override fun isSelectableDate(utcTimeMillis: Long) = utcTimeMillis.toUtcDate() in first..last
        },
    )
    DatePickerDialog(
        onDismissRequest = onDismiss,
        confirmButton = {
            TextButton(onClick = { onPick(state.selectedDateMillis?.toUtcDate() ?: first) }) { Text(stringResource(R.string.ok)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    ) {
        DatePicker(
            state,
            title = {
                Text(
                    stringResource(R.string.calendar_away_until_title),
                    modifier = Modifier.padding(start = 24.dp, end = 12.dp, top = 16.dp),
                )
            },
        )
    }
}

/** What each color says, and the mark of a day set from the app. */
@Composable
private fun Legend() {
    Column(Modifier.padding(top = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(Modifier.fillMaxWidth()) {
            Swatch(OfficeDay, R.string.calendar_office)
            Swatch(RemoteDay, R.string.calendar_remote)
        }
        Row(Modifier.fillMaxWidth()) {
            Swatch(AwayDay, R.string.calendar_away)
            Swatch(null, R.string.calendar_holiday)
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.width(14.dp), contentAlignment = Alignment.Center) {
                Box(
                    Modifier
                        .size(4.dp)
                        .clip(CircleShape)
                        .background(MaterialTheme.colorScheme.onSurface),
                )
            }
            Spacer(Modifier.width(8.dp))
            Text(stringResource(R.string.calendar_set_legend), style = MaterialTheme.typography.bodyMedium)
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            SchoolMark(MaterialTheme.colorScheme.onSurface)
            Spacer(Modifier.width(8.dp))
            Text(stringResource(R.string.calendar_school_legend), style = MaterialTheme.typography.bodyMedium)
        }
    }
}

/** The mark of a school holiday: a short bar across the top of the day. */
@Composable
private fun SchoolMark(color: Color, modifier: Modifier = Modifier) {
    Box(
        modifier
            .size(width = 14.dp, height = 3.dp)
            .clip(CircleShape)
            .background(color),
    )
}

/** A color and what it says; none for a public holiday, outlined as in the grid. */
@Composable
private fun RowScope.Swatch(fill: Color?, label: Int) {
    val shape = RoundedCornerShape(4.dp)
    Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
        Box(
            Modifier
                .size(14.dp)
                .clip(shape)
                .then(if (fill != null) Modifier.background(fill) else Modifier.border(1.dp, MaterialTheme.colorScheme.outline, shape)),
        )
        Spacer(Modifier.width(8.dp))
        Text(stringResource(label), style = MaterialTheme.typography.bodyMedium)
    }
}
