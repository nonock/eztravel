package io.github.nonock.eztravel.home

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

/** An eye: the backend watches the way. Tinted where shown. */
val Watching: ImageVector = eye(closed = false)

/** The eye struck through: nothing is watched now. */
val NotWatching: ImageVector = eye(closed = true)

private fun eye(closed: Boolean): ImageVector {
    val ink = SolidColor(Color.Black)
    return ImageVector.Builder(if (closed) "NotWatching" else "Watching", 24.dp, 24.dp, 24f, 24f)
        .addPath(addPathNodes("M2,12 C5,6.5 19,6.5 22,12 C19,17.5 5,17.5 2,12 Z"), stroke = ink, strokeLineWidth = 2f, strokeLineJoin = StrokeJoin.Round)
        .addPath(addPathNodes("M12,9 A3,3 0 1,1 12,15 A3,3 0 1,1 12,9 Z"), fill = ink)
        .apply { if (closed) addPath(addPathNodes("M4,4 L20,20"), stroke = ink, strokeLineWidth = 2f, strokeLineCap = StrokeCap.Round) }
        .build()
}
