package io.github.nonock.eztravel.settings

import androidx.annotation.StringRes
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.outlined.List
import androidx.compose.material.icons.outlined.DateRange
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material.icons.outlined.Warning
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.watchdog.Watchdog

/** The pages behind the gear: [label] names each in the list, [title] heads it, [about] says what it holds. */
enum class SettingsPage(@StringRes val label: Int, @StringRes val title: Int, @StringRes val about: Int, val icon: ImageVector) {
    CALENDAR(R.string.nav_calendar, R.string.nav_calendar, R.string.settings_calendar_about, Icons.Outlined.DateRange),
    USUAL_TRAINS(R.string.usual_title, R.string.usual_title, R.string.settings_usual_about, Icons.Outlined.Notifications),
    HISTORY(R.string.nav_history, R.string.history_title, R.string.settings_history_about, Icons.AutoMirrored.Outlined.List),
}

/**
 * The list behind the gear, each page a row to open; while the watchdog's
 * looks may come late, a last row opens Android's settings to allow them on time.
 */
@Composable
fun SettingsMenu(onOpen: (SettingsPage) -> Unit, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    // Read again each time the app comes back, as from Android's settings.
    var onTime by remember { mutableStateOf(Watchdog.exactAllowed(context)) }
    LifecycleResumeEffect(Unit) {
        onTime = Watchdog.exactAllowed(context)
        onPauseOrDispose {}
    }
    Column(modifier) {
        for (page in SettingsPage.entries) {
            MenuRow(page.icon, page.label, page.about) { onOpen(page) }
        }
        if (!onTime) {
            MenuRow(Icons.Outlined.Warning, R.string.settings_on_time, R.string.settings_on_time_about) {
                context.startActivity(Watchdog.allowExact(context))
            }
        }
    }
}

@Composable
private fun MenuRow(icon: ImageVector, @StringRes label: Int, @StringRes about: Int, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 72.dp)
            .clickable(onClick = onClick)
            .padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
        Spacer(Modifier.width(16.dp))
        Column(Modifier.weight(1f)) {
            Text(stringResource(label), style = MaterialTheme.typography.titleMedium)
            Text(
                stringResource(about),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Icon(
            Icons.AutoMirrored.Filled.KeyboardArrowRight,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
