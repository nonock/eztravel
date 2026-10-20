package io.github.nonock.eztravel.backend

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.time.LocalDate

/** The eztravel backend, as the app sees it. Tests replace it with a fake. */
interface Backend {
    /** False when this build has no backend address or no app key. */
    val configured: Boolean

    /** Blocking: call it off the main thread. */
    fun register(token: String)

    /** The last day away from the office, included, if any. */
    suspend fun awayThrough(): LocalDate?

    suspend fun setAwayThrough(date: LocalDate?)

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

    override suspend fun awayThrough(): LocalDate? = withContext(Dispatchers.IO) {
        val settings = JSONObject(request("GET", "/settings", null))
        // Android's org.json may read a null as the text "null": check for it first.
        if (settings.isNull("away_through")) null else LocalDate.parse(settings.getString("away_through"))
    }

    override suspend fun setAwayThrough(date: LocalDate?) = withContext(Dispatchers.IO) {
        request("PUT", "/settings", JSONObject().put("away_through", date?.toString() ?: JSONObject.NULL))
        Unit
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
        try {
            request("PUT", "/departure", JSONObject().put("journey", journey ?: JSONObject.NULL))
            null
        } catch (error: BackendError) {
            error.body?.takeIf { error.status == UNPROCESSABLE }?.let(Refusal::parse) ?: throw error
        }
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
