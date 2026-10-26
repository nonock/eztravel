package io.github.nonock.eztravel.settings

import android.app.Instrumentation
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.ScreenTesting
import io.github.nonock.eztravel.watchdog.Watchdog
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CopyOnWriteArrayList

@RunWith(AndroidJUnit4::class)
class SettingsMenuTest : ScreenTesting() {
    private val exact = Watchdog.exactAllowed

    @After
    fun putBack() {
        Watchdog.exactAllowed = exact
    }

    @Test
    fun whileTheWatchdogMayBeLateTheMenuOpensAndroidsSettingsForIt() {
        Watchdog.exactAllowed = { false }
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val started = CopyOnWriteArrayList<Intent>()
        // Android's page is not opened in the test: the intent is kept.
        val settings = object : Instrumentation.ActivityMonitor() {
            override fun onStartActivity(intent: Intent): Instrumentation.ActivityResult? =
                if (intent.action == Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM) {
                    started += intent
                    Instrumentation.ActivityResult(0, null)
                } else {
                    null
                }
        }
        instrumentation.addMonitor(settings)
        try {
            compose.setContent { SettingsMenu(onOpen = {}) }
            compose.onNodeWithText(text(R.string.settings_on_time_about)).assertIsDisplayed()
            compose.onNodeWithText(text(R.string.settings_on_time)).performClick()
            compose.waitUntil { started.isNotEmpty() }
            // For this app only.
            assertEquals(Uri.parse("package:${context.packageName}"), started.single().data)
        } finally {
            instrumentation.removeMonitor(settings)
        }
    }

    @Test
    fun withExactAlarmsTheMenuHoldsOnlyItsPages() {
        Watchdog.exactAllowed = { true }
        compose.setContent { SettingsMenu(onOpen = {}) }
        compose.onNodeWithText(text(R.string.nav_calendar)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.settings_on_time)).assertDoesNotExist()
    }

    @Test
    fun backFromAndroidsSettingsTheMenuSeesTheAlarmsAllowed() {
        var allowed = false
        Watchdog.exactAllowed = { allowed }
        val app = object : LifecycleOwner {
            val registry = LifecycleRegistry(this)
            override val lifecycle: Lifecycle get() = registry
        }
        compose.runOnUiThread { app.registry.currentState = Lifecycle.State.RESUMED }
        compose.setContent {
            CompositionLocalProvider(LocalLifecycleOwner provides app) { SettingsMenu(onOpen = {}) }
        }
        compose.onNodeWithText(text(R.string.settings_on_time)).assertIsDisplayed()
        // Away to Android's settings, allowed there, then back.
        compose.runOnUiThread { app.registry.currentState = Lifecycle.State.STARTED }
        allowed = true
        compose.runOnUiThread { app.registry.currentState = Lifecycle.State.RESUMED }
        compose.onNodeWithText(text(R.string.settings_on_time)).assertDoesNotExist()
    }
}
