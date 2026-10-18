package io.github.nonock.eztravel.backend

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AppBackendTest {
    private val original = AppBackend.current

    @After
    fun restore() {
        AppBackend.current = original
    }

    @Test
    fun thePhoneRegistersWithTheBackend() {
        val backend = FakeBackend()
        AppBackend.current = backend
        AppBackend.register("phone-token")
        assertEquals(listOf("phone-token"), backend.registered)
    }

    @Test
    fun aBuildWithoutServerRegistersNowhere() {
        val backend = FakeBackend(configured = false)
        AppBackend.current = backend
        AppBackend.register("phone-token")
        assertEquals(emptyList<String>(), backend.registered)
    }

    @Test
    fun aFailedRegistrationDoesNotStopTheApp() {
        val backend = FakeBackend(failing = true)
        AppBackend.current = backend
        AppBackend.register("phone-token")
        assertEquals(emptyList<String>(), backend.registered)
    }
}
