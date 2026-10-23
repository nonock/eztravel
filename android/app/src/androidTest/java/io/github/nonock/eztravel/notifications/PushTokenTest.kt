package io.github.nonock.eztravel.notifications

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

@RunWith(AndroidJUnit4::class)
class PushTokenTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    @Test
    fun firebaseGivesItsTokenWhereGoogleServicesRun() {
        // On a phone, not on an emulator built for tests, which has none.
        val google = runCatching { context.packageManager.getPackageInfo("com.google.android.gms", 0) }.isSuccess
        assumeTrue("needs Google Play services", google)
        val token = AtomicReference<String>()
        val given = CountDownLatch(1)
        PushToken.fetch { received ->
            token.set(received)
            given.countDown()
        }
        assertTrue(given.await(15, TimeUnit.SECONDS))
        assertTrue(token.get().isNotEmpty())
    }
}
