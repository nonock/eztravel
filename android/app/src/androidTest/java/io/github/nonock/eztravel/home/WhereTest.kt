package io.github.nonock.eztravel.home

import android.Manifest
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.GrantPermissionRule
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class WhereTest {
    @get:Rule
    val permission: GrantPermissionRule = GrantPermissionRule.grant(Manifest.permission.ACCESS_COARSE_LOCATION)

    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    @Test
    fun withoutLeaveThePhoneSaysNothingOfWhereItIs() = runBlocking {
        assertNull(context.position(allowed = false))
    }

    @Test
    fun allowedThePhoneSaysWhereItIsOrNothingWithinAFewSeconds() = runBlocking {
        val asked = System.currentTimeMillis()
        val here = context.position()
        // Somewhere on Earth, or nothing when the phone cannot tell: never a wait.
        assertTrue(here == null || here.lat in -90.0..90.0 && here.lon in -180.0..180.0)
        assertTrue(System.currentTimeMillis() - asked < 15_000)
    }
}
