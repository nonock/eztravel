package io.github.nonock.eztravel.backend

import com.sun.net.httpserver.HttpServer
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.File
import java.io.IOException
import java.net.InetSocketAddress
import java.time.LocalDate

class HttpBackendTest {
    private data class Received(val method: String, val path: String, val authorization: String?, val body: String)

    private val received = mutableListOf<Received>()
    private var status = 204
    private var answer = ""

    private val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0).apply {
        createContext("/") { exchange ->
            val body = exchange.requestBody.readBytes().decodeToString()
            received += Received(exchange.requestMethod, exchange.requestURI.path, exchange.requestHeaders.getFirst("Authorization"), body)
            val bytes = answer.toByteArray()
            exchange.sendResponseHeaders(status, if (bytes.isEmpty()) -1 else bytes.size.toLong())
            if (bytes.isNotEmpty()) exchange.responseBody.use { it.write(bytes) }
            exchange.close()
        }
        start()
    }

    private val backend = HttpBackend("http://127.0.0.1:${server.address.port}", "the-app-key")

    @After
    fun stop() = server.stop(0)

    @Test
    fun registersThePhoneWithTheAppKey() {
        backend.register("phone-token")
        val request = received.single()
        assertEquals(Received("POST", "/devices", "Bearer the-app-key", request.body), request)
        assertEquals("phone-token", JSONObject(request.body).getString("token"))
    }

    @Test
    fun readsTheTimeAway() = runBlocking {
        status = 200
        answer = """{"away_through":"2026-12-24"}"""
        assertEquals(LocalDate.of(2026, 12, 24), backend.awayThrough())
        answer = """{"away_through":null}"""
        assertNull(backend.awayThrough())
        assertEquals(listOf("GET"), received.map { it.method }.distinct())
    }

    @Test
    fun setsAndClearsTheTimeAway() = runBlocking {
        backend.setAwayThrough(LocalDate.of(2026, 12, 24))
        backend.setAwayThrough(null)
        assertEquals(listOf("PUT", "PUT"), received.map { it.method })
        assertEquals("2026-12-24", JSONObject(received[0].body).getString("away_through"))
        assertTrue(JSONObject(received[1].body).isNull("away_through"))
    }

    @Test
    fun readsTheStatus() = runBlocking {
        status = 200
        answer = File("../../fixtures/api/status-watching.json").readText()
        assertEquals(Status.parse(answer), backend.status())
        assertEquals(listOf("GET /status"), received.map { "${it.method} ${it.path}" })
    }

    @Test
    fun readsTheHistory() = runBlocking {
        status = 200
        answer = File("../../fixtures/api/history.json").readText()
        assertEquals(HistoryEntry.parse(answer), backend.history())
        assertEquals(listOf("GET /history"), received.map { "${it.method} ${it.path}" })
    }

    @Test
    fun readsTheDepartures() = runBlocking {
        status = 200
        answer = File("../../fixtures/api/departures.json").readText()
        assertEquals(Departures.parse(answer), backend.departures())
        assertEquals(listOf("GET /departures"), received.map { "${it.method} ${it.path}" })
    }

    @Test
    fun picksADepartureAndDropsIt() = runBlocking {
        assertNull(backend.pick("j2"))
        assertNull(backend.pick(null))
        assertEquals(listOf("PUT /departure"), received.map { "${it.method} ${it.path}" }.distinct())
        assertEquals("j2", JSONObject(received[0].body).getString("journey"))
        assertTrue(JSONObject(received[1].body).isNull("journey"))
    }

    @Test
    fun aRefusedPickSaysWhy() = runBlocking {
        status = 422
        answer = File("../../fixtures/api/pick-refused.json").readText()
        assertEquals(Refusal.UNAVAILABLE, backend.pick("j2"))
        // Without a reason it knows, or from a backend without picks: it was not saved.
        for ((code, body) in listOf(422 to "Unprocessable Entity", 405 to "", 404 to "", 500 to """{"refused":"unknown"}""")) {
            status = code
            answer = body
            try {
                backend.pick("j2")
                fail("a pick not saved must not pass silently")
            } catch (error: BackendError) {
                assertEquals(code, error.status)
            }
        }
    }

    @Test
    fun aRefusalIsAnError() {
        status = 401
        try {
            backend.register("phone-token")
            fail("a refusal must not pass silently")
        } catch (error: BackendError) {
            assertEquals(401, error.status)
        }
    }

    @Test(expected = IOException::class)
    fun anUnreachableBackendIsAnError() {
        server.stop(0)
        backend.register("phone-token")
    }

    @Test
    fun aBuildWithoutAddressOrKeyIsNotConfigured() {
        assertTrue(backend.configured)
        assertFalse(HttpBackend("", "the-app-key").configured)
        assertFalse(HttpBackend("https://example.org", "").configured)
    }
}
