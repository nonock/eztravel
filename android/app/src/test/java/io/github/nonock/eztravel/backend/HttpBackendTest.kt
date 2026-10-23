package io.github.nonock.eztravel.backend

import com.sun.net.httpserver.HttpServer
import io.github.nonock.eztravel.backend.Status.Direction
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
import java.time.LocalTime
import java.time.YearMonth

class HttpBackendTest {
    private data class Received(val method: String, val path: String, val authorization: String?, val body: String, val query: String? = null)

    private val received = mutableListOf<Received>()
    private var status = 204
    private var answer = ""

    private val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0).apply {
        createContext("/") { exchange ->
            val body = exchange.requestBody.readBytes().decodeToString()
            val uri = exchange.requestURI
            received += Received(exchange.requestMethod, uri.path, exchange.requestHeaders.getFirst("Authorization"), body, uri.query)
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
    fun readsTheUsualTrains() = runBlocking {
        status = 200
        answer = File("../../fixtures/api/settings.json").readText()
        assertEquals(UsualTrains(LocalTime.of(8, 7), LocalTime.of(17, 52)), backend.usualTrains())
        answer = """{"away_through":null,"usual_trains":{"outbound":null,"inbound":"17:52:00"}}"""
        assertEquals(UsualTrains(null, LocalTime.of(17, 52)), backend.usualTrains())
        // From a backend that has none.
        answer = """{"away_through":null}"""
        assertEquals(UsualTrains(null, null), backend.usualTrains())
        assertEquals(listOf("GET /settings"), received.map { "${it.method} ${it.path}" }.distinct())
    }

    @Test
    fun setsAndClearsAUsualTrain() = runBlocking {
        assertNull(backend.setUsualTrain(Direction.OUTBOUND, LocalTime.of(8, 7)))
        assertNull(backend.setUsualTrain(Direction.INBOUND, null))
        assertEquals(listOf("PUT /usual-train"), received.map { "${it.method} ${it.path}" }.distinct())
        val (first, second) = received.map { JSONObject(it.body) }
        assertEquals("outbound" to "08:07", first.getString("direction") to first.getString("time"))
        assertEquals("inbound", second.getString("direction"))
        assertTrue(second.isNull("time"))
    }

    @Test
    fun aRefusedUsualTrainSaysWhy() = runBlocking {
        status = 422
        answer = """{"refused":"outside_window"}"""
        assertEquals(Refusal.OUTSIDE_WINDOW, backend.setUsualTrain(Direction.OUTBOUND, LocalTime.of(10, 0)))
        status = 404
        answer = ""
        try {
            backend.setUsualTrain(Direction.OUTBOUND, LocalTime.of(8, 7))
            fail("a usual train not saved must not pass silently")
        } catch (error: BackendError) {
            assertEquals(404, error.status)
        }
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
    fun readsTheAlerts() = runBlocking {
        status = 200
        answer = File("../../fixtures/api/alerts.json").readText()
        assertEquals(Alerts.parse(answer), backend.alerts())
        assertEquals(listOf("GET /alerts"), received.map { "${it.method} ${it.path}" })
    }

    @Test
    fun readsAMonthOfTheCalendar() = runBlocking {
        status = 200
        answer = File("../../fixtures/api/calendar.json").readText()
        assertEquals(CalendarDay.parse(answer), backend.month(YearMonth.of(2026, 11)))
        assertEquals(listOf("GET /calendar month=2026-11"), received.map { "${it.method} ${it.path} ${it.query}" })
    }

    @Test
    fun setsADayAndPutsItBack() = runBlocking {
        assertNull(backend.setDay(LocalDate.of(2026, 11, 5), CalendarDay.Kind.REMOTE))
        assertNull(backend.setDay(LocalDate.of(2026, 11, 5), null))
        assertEquals(listOf("PUT /calendar"), received.map { "${it.method} ${it.path}" }.distinct())
        val (first, second) = received.map { JSONObject(it.body) }
        assertEquals("2026-11-05" to "remote", first.getString("date") to first.getString("kind"))
        assertTrue(second.isNull("kind"))
    }

    @Test
    fun aDayThatCannotBeSetSaysWhy() = runBlocking {
        status = 422
        answer = File("../../fixtures/api/day-refused.json").readText()
        assertEquals(DayRefusal.PAST, backend.setDay(LocalDate.of(2020, 1, 6), CalendarDay.Kind.OFFICE))
        answer = """{"refused":"too_far"}"""
        assertEquals(DayRefusal.TOO_FAR, backend.setDay(LocalDate.of(2100, 1, 4), null))
        // Without a reason it knows, or from a backend without the calendar: it was not saved.
        for ((code, body) in listOf(422 to """{"refused":"outside_window"}""", 422 to "not JSON", 404 to "")) {
            status = code
            answer = body
            try {
                backend.setDay(LocalDate.of(2026, 11, 5), null)
                fail("a day not saved must not pass silently")
            } catch (error: BackendError) {
                assertEquals(code, error.status)
            }
        }
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
