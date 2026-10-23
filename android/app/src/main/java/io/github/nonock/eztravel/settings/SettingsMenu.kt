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
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.github.nonock.eztravel.R

/** The pages behind the gear: [label] names each in the list, [title] heads it, [about] says what it holds. */
enum class SettingsPage(@StringRes val label: Int, @StringRes val title: Int, @StringRes val about: Int, val icon: ImageVector) {
    CALENDAR(R.string.nav_calendar, R.string.nav_calendar, R.string.settings_calendar_about, Icons.Outlined.DateRange),
    USUAL_TRAINS(R.string.usual_title, R.string.usual_title, R.string.settings_usual_about, Icons.Outlined.Notifications),
    HISTORY(R.string.nav_history, R.string.history_title, R.string.settings_history_about, Icons.AutoMirrored.Outlined.List),
}

/** The list behind the gear, each page a row to open. */
@Composable
fun SettingsMenu(onOpen: (SettingsPage) -> Unit, modifier: Modifier = Modifier) {
    Column(modifier) {
        for (page in SettingsPage.entries) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = 72.dp)
                    .clickable { onOpen(page) }
                    .padding(vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(page.icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(16.dp))
                Column(Modifier.weight(1f)) {
                    Text(stringResource(page.label), style = MaterialTheme.typography.titleMedium)
                    Text(
                        stringResource(page.about),
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
    }
}
