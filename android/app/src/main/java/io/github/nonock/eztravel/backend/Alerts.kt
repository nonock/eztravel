package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.notifications.PushMessage
import org.json.JSONArray
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

        /** The title to unfold for a notification's record: by its [record] id, else its [title]; null once gone. */
        fun unfolded(record: String?, title: String?): String? =
            (alerts.firstOrNull { it.id == record } ?: alerts.firstOrNull { it.title == title })?.title

        /** How many are under way, as shown: one per title. */
        val underWay: Int
            get() = grouped.count { (alert, _) -> alert.active }

        /** How many are to come, as shown. */
        val toCome: Int
            get() = grouped.size - underWay

        /** The worst of those under way, if any: it shows even when the line is folded. */
        val worstUnderWay: Severity?
            get() = alerts.filter { it.active }.minOfOrNull { it.severity }
    }

    enum class Severity { BLOCKING, DISRUPTED, INFORMATION }

    enum class Cause { WORKS, INCIDENT, INFORMATION, UNKNOWN }

    /**
     * [begin] and [end] are those of the period under way when [active], or
     * else of the next one; [onWay] when it touches the commute, as notified;
     * [links] are on words of [text].
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
        val links: List<Link> = emptyList(),
    )

    /** A web link on the chars of a text from [start] to before [end], as Kotlin counts them; always `https`. */
    data class Link(val start: Int, val end: Int, val url: String)

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
            links = links(json.optJSONArray("links"), json.getString("text")),
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

        /** The backend counts code points; any link not safe to open, or off the text, is left out. */
        private fun links(json: JSONArray?, text: String): List<Link> {
            val length = text.codePointCount(0, text.length)
            return json?.objects().orEmpty().mapNotNull { link ->
                val start = link.optInt("start", -1)
                val end = link.optInt("end", -1)
                val url = link.optString("url")
                if (start in 0 until end && end <= length && url.startsWith("https://")) {
                    Link(text.offsetByCodePoints(0, start), text.offsetByCodePoints(0, end), url)
                } else {
                    null
                }
            }
        }
    }
}
