package io.github.nonock.eztravel

import android.content.ComponentName
import android.content.res.TypedArray
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.drawable.AdaptiveIconDrawable
import android.graphics.drawable.ColorDrawable
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import androidx.core.splashscreen.R as SplashR

/**
 * The "Z route" logo: a Z drawn as a ride, from a first stop to a last one.
 * Pixels are looked up in the icons' own units: 108 for launcher layers, 64 for the notification icon.
 */
@RunWith(AndroidJUnit4::class)
class BrandingTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    /** [id] drawn on a [size]-pixel square, to look at its pixels. */
    private fun draw(id: Int, size: Int): Bitmap {
        val drawable = context.getDrawable(id)!!
        val bitmap = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        drawable.setBounds(0, 0, size, size)
        drawable.draw(Canvas(bitmap))
        return bitmap
    }

    /** The pixel at (`x`, `y`) in units of a `units`-wide icon, offset by `origin`. */
    private fun Bitmap.at(x: Float, y: Float, units: Float, origin: Float = 0f): Int =
        getPixel(((x - origin) / units * width).toInt(), ((y - origin) / units * height).toInt())

    private fun opaque(pixel: Int) = Color.alpha(pixel) > 200

    private fun clear(pixel: Int) = Color.alpha(pixel) < 50

    @Test
    fun theLauncherIconIsAdaptiveOnNavyWithAThemedVersion() {
        assertEquals(R.mipmap.ic_launcher, context.applicationInfo.icon)
        val icon = context.getDrawable(R.mipmap.ic_launcher) as AdaptiveIconDrawable
        assertEquals(context.getColor(R.color.brand_navy), (icon.background as ColorDrawable).color)
        assertNotNull(icon.monochrome)
    }

    @Test
    fun theIconDrawsAnAmberRideBetweenWhiteStops() {
        val icon = draw(R.drawable.ic_launcher_foreground, 216)
        assertEquals(Color.WHITE, icon.at(36f, 38f, 108f))
        assertEquals(Color.WHITE, icon.at(72f, 70f, 108f))
        assertEquals(context.getColor(R.color.brand_amber), icon.at(58f, 38f, 108f))
        assertEquals(context.getColor(R.color.brand_amber), icon.at(54f, 54f, 108f))
    }

    @Test
    fun singleColorIconsKeepTheirStopsHollow() {
        // The system paints these in one color: only transparency can draw a stop.
        for ((icon, units, origin) in listOf(
            Triple(draw(R.drawable.ic_launcher_monochrome, 216), 108f, 0f),
            Triple(draw(R.drawable.ic_notification, 128), 64f, 22f),
        )) {
            fun pixel(x: Float, y: Float) = icon.at(x, y, units, origin)
            assertTrue("first stop", clear(pixel(36f, 38f)))
            assertTrue("last stop", clear(pixel(72f, 70f)))
            assertTrue("ring of the first stop", opaque(pixel(43.5f, 38f)))
            assertTrue("ride", opaque(pixel(58f, 38f)))
            assertTrue("diagonal", opaque(pixel(54f, 54f)))
            assertTrue("inside the Z", clear(pixel(40f, 54f)))
        }
    }

    private fun <T> startingTheme(attribute: Int, read: (TypedArray) -> T): T {
        val values = context.obtainStyledAttributes(R.style.Theme_Eztravel_Starting, intArrayOf(attribute))
        try {
            return read(values)
        } finally {
            values.recycle()
        }
    }

    @Test
    fun theAppStartsOnTheIconOverNavyThenTakesItsOwnTheme() {
        val activity = context.packageManager.getActivityInfo(ComponentName(context, MainActivity::class.java), 0)
        assertEquals(R.style.Theme_Eztravel_Starting, activity.themeResource)
        assertEquals(
            context.getColor(R.color.brand_navy),
            startingTheme(SplashR.attr.windowSplashScreenBackground) { it.getColor(0, 0) },
        )
        assertEquals(
            R.drawable.ic_launcher_foreground,
            startingTheme(SplashR.attr.windowSplashScreenAnimatedIcon) { it.getResourceId(0, 0) },
        )
        assertEquals(R.style.Theme_Eztravel, startingTheme(SplashR.attr.postSplashScreenTheme) { it.getResourceId(0, 0) })
    }
}
