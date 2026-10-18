package io.github.nonock.eztravel.settings

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Button
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Backend
import kotlinx.coroutines.launch
import org.json.JSONException
import java.io.IOException
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

private sealed interface Away {
    data object Loading : Away
    data object NotConfigured : Away
    data object Unreachable : Away
    data class Known(val lastDay: LocalDate?) : Away
}

/** The last day away from the office: no notification until then. */
@Composable
fun AwaySetting(backend: Backend, modifier: Modifier = Modifier) {
    var away by remember { mutableStateOf<Away>(Away.Loading) }
    var choosing by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(backend) {
        away = if (backend.configured) reach { Away.Known(backend.awayThrough()) } else Away.NotConfigured
    }

    fun save(lastDay: LocalDate?) {
        scope.launch {
            away = reach {
                backend.setAwayThrough(lastDay)
                Away.Known(lastDay)
            }
        }
    }

    Column(modifier) {
        Text(stringResource(R.string.away_title), style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        when (val current = away) {
            Away.Loading -> Text(stringResource(R.string.away_loading))
            Away.NotConfigured -> Text(stringResource(R.string.away_not_configured))
            Away.Unreachable -> Text(stringResource(R.string.away_unreachable))
            is Away.Known -> {
                val lastDay = current.lastDay
                Text(
                    if (lastDay == null) {
                        stringResource(R.string.away_none)
                    } else {
                        stringResource(R.string.away_until, DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL).format(lastDay))
                    },
                )
                Spacer(Modifier.height(8.dp))
                Row {
                    Button(onClick = { choosing = true }) { Text(stringResource(R.string.away_choose)) }
                    if (lastDay != null) {
                        Spacer(Modifier.width(8.dp))
                        TextButton(onClick = { save(null) }) { Text(stringResource(R.string.away_clear)) }
                    }
                }
                if (choosing) {
                    LastDayPicker(
                        initial = lastDay ?: LocalDate.now(),
                        onPick = { day ->
                            choosing = false
                            save(day)
                        },
                        onDismiss = { choosing = false },
                    )
                }
            }
        }
    }
}

/** A call to the backend, or `Unreachable` when it fails. */
private suspend fun reach(call: suspend () -> Away): Away = try {
    call()
} catch (_: IOException) {
    Away.Unreachable
} catch (_: JSONException) {
    Away.Unreachable
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun LastDayPicker(initial: LocalDate, onPick: (LocalDate) -> Unit, onDismiss: () -> Unit) {
    val today = LocalDate.now()
    val state = rememberDatePickerState(
        initialSelectedDateMillis = initial.toUtcMillis(),
        selectableDates = object : SelectableDates {
            override fun isSelectableDate(utcTimeMillis: Long) = !utcTimeMillis.toUtcDate().isBefore(today)
        },
    )
    DatePickerDialog(
        onDismissRequest = onDismiss,
        confirmButton = {
            TextButton(onClick = { state.selectedDateMillis?.let { onPick(it.toUtcDate()) } }) { Text(stringResource(R.string.ok)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    ) {
        DatePicker(state)
    }
}
