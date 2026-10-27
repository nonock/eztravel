package io.github.nonock.eztravel

import android.content.Context
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.dp
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Rule

/**
 * A screen under test: its compose rule, the app's words, and waiting for what
 * the screen reads. Whatever a test ends on is checked for whoever cannot see
 * it well or tap it finely.
 */
abstract class ScreenTesting {
    @get:Rule
    val compose = createComposeRule()

    protected val context: Context = InstrumentationRegistry.getInstrumentation().targetContext

    protected fun text(id: Int, vararg args: Any): String = context.getString(id, *args)

    /** A screen reads the backend once resumed: wait for [matcher], then check that it shows. */
    protected fun waitFor(matcher: SemanticsMatcher) {
        compose.waitUntil(timeoutMillis = 5_000) {
            compose.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNode(matcher).assertIsDisplayed()
    }

    protected fun waitFor(text: String) = waitFor(hasText(text))

    @After
    fun everyTapTargetIsNamedAndLargeEnough() = compose.checkTapTargets()
}

/**
 * Each element on screen that takes a tap says what it is, aloud, and offers
 * at least 48 dp to a finger; a link within a text is exempt, as it runs with
 * the words around it.
 */
fun ComposeTestRule.checkTapTargets() {
    val targets = try {
        onAllNodes(hasClickAction()).fetchSemanticsNodes()
    } catch (_: IllegalStateException) {
        // Nothing composed.
        return
    }
    val least = with(density) { 48.dp.toPx() } - 0.5f
    for (target in targets) {
        val config = target.config
        if (SemanticsProperties.LinkTestMarker in config) continue
        // Wholly on screen only: what is scrolled away, even in part, or folded, has its touch area cut.
        val shown = target.boundsInRoot
        val whole = target.size.width > 0 && target.size.height > 0 &&
            shown.width >= target.size.width - 1 && shown.height >= target.size.height - 1
        if (!whole) continue
        val words = config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text } +
            config.getOrNull(SemanticsProperties.ContentDescription).orEmpty()
        assertTrue("a tap target says nothing: $config", words.any { it.isNotBlank() })
        val touch = target.touchBoundsInRoot
        assertTrue(
            "too small to tap, ${touch.width.toInt()} x ${touch.height.toInt()} px: ${words.joinToString(" ")}",
            touch.width >= least && touch.height >= least,
        )
    }
}
