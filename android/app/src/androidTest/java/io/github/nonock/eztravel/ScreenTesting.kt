package io.github.nonock.eztravel

import android.content.Context
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Rule

/** A screen under test: its compose rule, the app's words, and waiting for what the screen reads. */
abstract class ScreenTest {
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
}
