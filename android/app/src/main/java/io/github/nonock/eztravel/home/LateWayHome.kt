package io.github.nonock.eztravel.home

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Backend
import io.github.nonock.eztravel.backend.Refusal
import io.github.nonock.eztravel.backend.Status
import io.github.nonock.eztravel.settings.TimeDialog
import kotlinx.coroutines.launch
import java.io.IOException
import java.time.LocalTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.temporal.ChronoUnit

/**
 * Today's way home when it is later than usual, as the backend has it in
 * [late], and the way to set it: the evening is then watched until an hour
 * after leaving, and the usual train is not reminded. `onSaved` follows each
 * change saved, for the status to be read again.
 */
@Composable
fun LateWayHome(
    backend: Backend,
    late: Status.Late?,
    modifier: Modifier = Modifier,
    now: () -> LocalTime = LocalTime::now,
    onSaved: () -> Unit = {},
) {
    var choosing by remember { mutableStateOf(false) }
    // Why the last change was not saved.
    var failed by remember { mutableStateOf<Int?>(null) }
    val scope = rememberCoroutineScope()

    fun save(leave: LocalTime?) {
        scope.launch {
            failed = try {
                backend.setLate(leave)?.lateMessage().also { if (it == null) onSaved() }
            } catch (_: IOException) {
                R.string.late_failed
            }
        }
    }

    Column(modifier) {
        if (late == null) {
            OutlinedButton(onClick = { choosing = true }) { Text(stringResource(R.string.late_button)) }
        } else {
            Text(stringResource(R.string.late_set, late.leave.shown(), late.until.shown()), style = MaterialTheme.typography.bodyLarge)
            Row(verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = { choosing = true }) { Text(stringResource(R.string.late_change)) }
                TextButton(onClick = { save(null) }) { Text(stringResource(R.string.late_as_usual)) }
            }
        }
        failed?.let { Text(stringResource(it), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(top = 4.dp)) }
    }
    if (choosing) {
        TimeDialog(
            title = R.string.late_pick_title,
            // From the next full hour, unless a time is set already.
            initial = late?.leave ?: now().truncatedTo(ChronoUnit.HOURS).plusHours(1),
            onPick = { leave ->
                choosing = false
                save(leave)
            },
            onDismiss = { choosing = false },
        )
    }
}

private fun LocalTime.shown(): String = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).format(this)

private fun Refusal.lateMessage() =
    if (this == Refusal.NOT_OFFICE_DAY) R.string.late_refused_not_office_day else R.string.late_refused_outside_window
