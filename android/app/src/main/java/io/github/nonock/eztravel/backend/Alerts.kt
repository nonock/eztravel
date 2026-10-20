package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.notifications.PushMessage
import org.json.JSONObject
import java.time.Instant

/**
 * Every disruption of the lines that is not over yet; [lines] is null when the
 * backend cannot read them. The format is fixed by `fixtures/api/alerts.json`
 * in the repository, which the backend tests read too.
 */
data class Alerts(val lines: List<LineAlerts>?) {
    /** A line's alerts: under way first, the worst first, then those to come. */
    data class LineAlerts(val line: PushMessage.Line, val color: Int?, val alerts: List<Alert>) {
        /** One per title, the first as listed, with how many more share it: works come split by period. */
        val grouped: List<Pair<Alert, Int>>
            get() = alerts.groupBy { it.title }.map { (_, same) -> same.first() to same.size - 1 }
    }

    enum class Severity { BLOCKING, DISRUPTED, INFORMATION }

    enum class Cause { WORKS, INCIDENT, INFORMATION, UNKNOWN }

    /**
     * [begin] and [end] are those of the period under way when [active], or
     * else of the next one; [onWay] when it touches the commute, as notified.
     */
    data class Alert(
        val id: String,
        val severity: Severity,
        val cause: Cause,
        val title: String,
        val text: String,
        val active: Boolean,
        val begin: Instant,
        val end: Instant,
        val onWay: Boolean,
        val updated: Instant,
    )

    companion object {
        /** Values a newer backend may add fall back on the safe side: trouble stays trouble. */
        fun parse(json: String): Alerts {
            val root = JSONObject(json)
            if (root.isNull("lines")) return Alerts(null)
            return Alerts(
                root.getJSONArray("lines").objects().map { line ->
                    LineAlerts(
                        PushMessage.Line(line.getString("line"), PushMessage.mode(line.getString("mode"))),
                        line.stringOrNull("color")?.let(::hexColor),
                        line.getJSONArray("alerts").objects().map(::alert),
                    )
                },
            )
        }

        private fun alert(json: JSONObject) = Alert(
            id = json.getString("id"),
            severity = when (json.getString("severity")) {
                "blocking" -> Severity.BLOCKING
                "information" -> Severity.INFORMATION
                else -> Severity.DISRUPTED
            },
            cause = when (json.getString("cause")) {
                "works" -> Cause.WORKS
                "incident" -> Cause.INCIDENT
                "information" -> Cause.INFORMATION
                else -> Cause.UNKNOWN
            },
            title = json.getString("title"),
            text = json.getString("text"),
            active = json.optBoolean("active"),
            begin = Instant.parse(json.getString("begin")),
            end = Instant.parse(json.getString("end")),
            onWay = json.optBoolean("on_way"),
            updated = Instant.parse(json.getString("updated")),
        )
    }
}
