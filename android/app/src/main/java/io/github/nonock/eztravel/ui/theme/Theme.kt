package io.github.nonock.eztravel.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/** Always dark, from the logo: navy grounds, amber accent. Line colors are the other cues. */
val EztravelColors = darkColorScheme(
    primary = Amber,
    onPrimary = Navy,
    primaryContainer = Color(0xFF5C3F00),
    onPrimaryContainer = Color(0xFFFFDDB0),
    secondary = Color(0xFFB9C7DB),
    onSecondary = Navy,
    tertiary = Color(0xFF8FD3FF),
    onTertiary = Navy,
    background = Navy,
    onBackground = Color(0xFFE7EBF1),
    surface = Navy,
    onSurface = Color(0xFFE7EBF1),
    surfaceVariant = Color(0xFF22324A),
    onSurfaceVariant = Color(0xFFB4C0D0),
    surfaceContainerLowest = Color(0xFF0A1422),
    surfaceContainerLow = Color(0xFF122135),
    surfaceContainer = Color(0xFF16263C),
    surfaceContainerHigh = Color(0xFF1C2D45),
    surfaceContainerHighest = Color(0xFF23354F),
    outline = Color(0xFF6B7A90),
    outlineVariant = Color(0xFF34445C),
    error = Color(0xFFFF8F85),
    onError = Color(0xFF410002),
)

@Composable
fun EztravelTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = EztravelColors, typography = Typography, content = content)
}
