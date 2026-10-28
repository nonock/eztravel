package io.github.nonock.eztravel.backend

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.time.LocalDate
import java.time.LocalTime
import java.time.YearMonth

/** The eztravel backend, as the app sees it. Tests replace it with a fake. */
interface Backend {
    /** False when this build has no backend address or no app key. */
    val configured: Boolean

    /** Blocking: call it off the main thread. */
    fun register(token: String)

    /** The usual train of each way. */
    suspend fun usualTrains(): UsualTrains

    /** Sets the usual train of [direction], or none: null once saved, or why the backend refuses it. */
    suspend fun setUsualTrain(direction: Status.Direction, time: LocalTime?): Refusal?

    /**
     * Sets today's way home later than usual, leaving at [leave], or as usual
     * with null: null once saved, or why the backend refuses it.
     */
    suspend fun setLate(leave: LocalTime?): Refusal?

    /** Each day of [month], what it is and why. */
    suspend fun month(month: YearMonth): List<CalendarDay>

    /**
     * Sets each day from [first] through [last] to [kind], office, remote or
     * away, or back as planned with null: null once saved, or why the backend
     * refuses it.
     */
    suspend fun setDays(first: LocalDate, last: LocalDate, kind: CalendarDay.Kind?): DayRefusal?

    /** What the backend sees: the lines being watched and whether checks work. */
    suspend fun status(): Status

    /** What the backend sent, newest first. */
    suspend fun history(): List<HistoryEntry>

    /** The next departures on the way shown. */
    suspend fun departures(): Departures

    /** Every disruption of the lines that is not over yet. */
    suspend fun alerts(): Alerts

    /**
     * Picks the train [journey] for the reminder to leave, or none: null once
     * saved, or why the backend refuses it.
     */
    suspend fun pick(journey: String?): Refusal?
}

/** [body] is the backend's answer, when it gave one. */
class BackendError(val status: Int, val body: String? = null) : IOException("the backend answered $status")

class HttpBackend(private val baseUrl: String, private val key: String) : Backend {
    override val configured: Boolean
        get() = baseUrl.isNotEmpty() && key.isNotEmpty()

    override fun register(token: String) {
        request("POST", "/devices", JSONObject().put("token", token))
    }

    override suspend fun usualTrains(): UsualTrains = withContext(Dispatchers.IO) {
        UsualTrains.parse(request("GET", "/settings", null))
    }

    override suspend fun setUsualTrain(direction: Status.Direction, time: LocalTime?): Refusal? = withContext(Dispatchers.IO) {
        val usual = JSONObject().put("direction", direction.name.lowercase()).put("time", time?.toString() ?: JSONObject.NULL)
        refusable("PUT", "/usual-train", usual, Refusal::parse)
    }

    override suspend fun setLate(leave: LocalTime?): Refusal? = withContext(Dispatchers.IO) {
        refusable("PUT", "/late", JSONObject().put("leave", leave?.toString() ?: JSONObject.NULL), Refusal::parse)
    }

    override suspend fun month(month: YearMonth): List<CalendarDay> = withContext(Dispatchers.IO) {
        CalendarDay.parse(request("GET", "/calendar?month=$month", null))
    }

    override suspend fun setDays(first: LocalDate, last: LocalDate, kind: CalendarDay.Kind?): DayRefusal? = withContext(Dispatchers.IO) {
        val days = JSONObject().put("date", first.toString()).put("kind", kind?.name?.lowercase() ?: JSONObject.NULL)
        // A single day without it, as the first backend with the calendar takes it.
        if (last != first) days.put("through", last.toString())
        refusable("PUT", "/calendar", days, DayRefusal::parse)
    }

    override suspend fun status(): Status = withContext(Dispatchers.IO) {
        Status.parse(request("GET", "/status", null))
    }

    override suspend fun history(): List<HistoryEntry> = withContext(Dispatchers.IO) {
        HistoryEntry.parse(request("GET", "/history", null))
    }

    override suspend fun departures(): Departures = withContext(Dispatchers.IO) {
        Departures.parse(request("GET", "/departures", null))
    }

    override suspend fun alerts(): Alerts = withContext(Dispatchers.IO) {
        Alerts.parse(request("GET", "/alerts", null))
    }

    override suspend fun pick(journey: String?): Refusal? = withContext(Dispatchers.IO) {
        refusable("PUT", "/departure", JSONObject().put("journey", journey ?: JSONObject.NULL), Refusal::parse)
    }

    /** Null once saved, or why the backend refuses it, as [reason] reads it; any other failure throws. */
    private fun <R> refusable(method: String, path: String, body: JSONObject, reason: (String) -> R?): R? = try {
        request(method, path, body)
        null
    } catch (error: BackendError) {
        error.body?.takeIf { error.status == UNPROCESSABLE }?.let(reason) ?: throw error
    }

    private fun request(method: String, path: String, body: JSONObject?): String {
        val connection = URL(baseUrl + path).openConnection() as HttpURLConnection
        try {
            connection.requestMethod = method
            connection.connectTimeout = TIMEOUT_MS
            connection.readTimeout = TIMEOUT_MS
            connection.setRequestProperty("Authorization", "Bearer $key")
            if (body != null) {
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/json")
                connection.outputStream.use { it.write(body.toString().toByteArray()) }
            }
            val status = connection.responseCode
            if (status !in 200..299) throw BackendError(status, connection.errorStream?.bufferedReader()?.use { it.readText() })
            return connection.inputStream.bufferedReader().use { it.readText() }
        } finally {
            connection.disconnect()
        }
    }

    private companion object {
        const val TIMEOUT_MS = 15_000
        const val UNPROCESSABLE = 422
    }
}
