package io.github.nonock.eztravel.home

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.nonock.eztravel.notifications.PushMessage
import io.github.nonock.eztravel.notifications.label

/** Black or white, whichever contrasts more with [background]. */
fun contentOn(background: Color): Color = if (background.luminance() > 0.179f) Color.Black else Color.White

/**
 * The line's name on its color: round for the metro, square for trains, as on
 * the network's maps. Read aloud as "Line J" or "Metro 12".
 */
@Composable
fun LineBadge(line: PushMessage.Line, color: Int?, modifier: Modifier = Modifier) {
    val ground = color?.let { Color(it) } ?: MaterialTheme.colorScheme.surfaceVariant
    val shape = if (line.mode == PushMessage.Mode.METRO) CircleShape else RoundedCornerShape(6.dp)
    val label = line.label(LocalResources.current)
    Box(
        modifier
            .size(32.dp)
            .background(ground, shape)
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            line.name,
            color = contentOn(ground),
            style = MaterialTheme.typography.labelLarge,
            fontWeight = FontWeight.Bold,
            maxLines = 1,
            modifier = Modifier.clearAndSetSemantics {},
        )
    }
}
