package io.github.nonock.eztravel.settings

import android.text.format.DateFormat
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimeInput
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Backend
import io.github.nonock.eztravel.backend.Refusal
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.backend.UsualTrains
import kotlinx.coroutines.launch
import org.json.JSONException
import java.io.IOException
import java.time.LocalTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

private sealed interface Usual {
    data object Loading : Usual
    data object Unreachable : Usual
    data class Known(val trains: UsualTrains) : Usual
}

/**
 * The usual train of each way, by its time: on office days, the reminder to
 * leave is for the first train from then. `onSaved` follows each change saved.
 * A build without a server shows nothing: the time away says why.
 */
@Composable
fun UsualTrainSetting(backend: Backend, modifier: Modifier = Modifier, onSaved: () -> Unit = {}) {
    if (!backend.configured) return
    var usual by remember { mutableStateOf<Usual>(Usual.Loading) }
    var choosing by remember { mutableStateOf<Direction?>(null) }
    var refused by remember { mutableStateOf<Refusal?>(null) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(backend) {
        usual = reach { Usual.Known(backend.usualTrains()) }
    }

    fun save(trains: UsualTrains, direction: Direction, time: LocalTime?) {
        scope.launch {
            refused = null
            usual = reach {
                val refusal = backend.setUsualTrain(direction, time)
                refused = refusal
                if (refusal != null) {
                    Usual.Known(trains)
                } else {
                    onSaved()
                    Usual.Known(trains.with(direction, time))
                }
            }
        }
    }

    Column(modifier) {
        Text(stringResource(R.string.usual_title), style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(4.dp))
        Text(stringResource(R.string.usual_explained), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        when (val current = usual) {
            Usual.Loading -> Text(stringResource(R.string.away_loading), modifier = Modifier.padding(top = 8.dp))
            Usual.Unreachable -> Text(stringResource(R.string.usual_unreachable), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(top = 8.dp))
            is Usual.Known -> {
                for (direction in Direction.entries) {
                    Way(direction, current.trains.of(direction), onChoose = { choosing = direction }, onClear = { save(current.trains, direction, null) })
                }
                refused?.let { Text(stringResource(it.usualMessage()), color = MaterialTheme.colorScheme.error) }
                choosing?.let { direction ->
                    TrainTime(
                        initial = current.trains.of(direction) ?: if (direction == Direction.OUTBOUND) MORNING else EVENING,
                        onPick = { time ->
                            choosing = null
                            save(current.trains, direction, time)
                        },
                        onDismiss = { choosing = null },
                    )
                }
            }
        }
    }
}

/** Where the picker opens without a usual train. */
private val MORNING = LocalTime.of(8, 0)
private val EVENING = LocalTime.of(18, 0)

private fun Refusal.usualMessage() = if (this == Refusal.OUTSIDE_WINDOW) R.string.departures_refused_outside_window else R.string.usual_refused

/** A call to the backend, or `Unreachable` when it fails. */
private suspend fun reach(call: suspend () -> Usual): Usual = try {
    call()
} catch (_: IOException) {
    Usual.Unreachable
} catch (_: JSONException) {
    Usual.Unreachable
}

/** One way, read as one and tapped to choose its train: "To work, 8:07". */
@Composable
private fun Way(direction: Direction, time: LocalTime?, onChoose: () -> Unit, onClear: () -> Unit) {
    val (label, choose, clear) = when (direction) {
        Direction.OUTBOUND -> Triple(R.string.usual_outbound, R.string.usual_choose_outbound, R.string.usual_clear_outbound)
        Direction.INBOUND -> Triple(R.string.usual_inbound, R.string.usual_choose_inbound, R.string.usual_clear_inbound)
    }
    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = 8.dp)
            .clip(RoundedCornerShape(12.dp))
            .clickable(onClickLabel = stringResource(choose), onClick = onChoose)
            .padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(stringResource(label), style = MaterialTheme.typography.bodyLarge)
            if (time == null) {
                Text(stringResource(R.string.usual_none), color = MaterialTheme.colorScheme.onSurfaceVariant)
            } else {
                Text(
                    DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).format(time),
                    style = MaterialTheme.typography.titleMedium.copy(fontFeatureSettings = "tnum"),
                    color = MaterialTheme.colorScheme.primary,
                )
            }
        }
        if (time != null) {
            IconButton(onClick = onClear) {
                Icon(Icons.Filled.Close, contentDescription = stringResource(clear), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

/** The train's time, typed: exact minutes are quicker to type than to set on a dial. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TrainTime(initial: LocalTime, onPick: (LocalTime) -> Unit, onDismiss: () -> Unit) {
    val state = rememberTimePickerState(initial.hour, initial.minute, is24Hour = DateFormat.is24HourFormat(LocalContext.current))
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { TextButton(onClick = { onPick(LocalTime.of(state.hour, state.minute)) }) { Text(stringResource(R.string.ok)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
        title = { Text(stringResource(R.string.usual_pick_title)) },
        text = { TimeInput(state) },
    )
}
