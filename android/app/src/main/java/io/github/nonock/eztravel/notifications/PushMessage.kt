package io.github.nonock.eztravel.notifications

import java.time.Instant

/**
 * A push message from the backend. Its format is fixed by `fixtures/push/` in
 * the repository, which the backend tests read too.
 */
sealed interface PushMessage {
    data class Line(val name: String, val mode: Mode)

    enum class Mode { TRAIN, METRO, OTHER }

    enum class Severity { BLOCKING, DISRUPTED }

    /** `title` and `text` are the operator's own words, in French, as published. */
    data class Started(
        val line: Line,
        val severity: Severity,
        val title: String?,
        val text: String?,
        override val until: Instant? = null,
    ) : PushMessage

    data class Changed(
        val line: Line,
        val severity: Severity,
        val previous: Severity,
        val title: String?,
        val text: String?,
        override val until: Instant? = null,
    ) : PushMessage

    data class Ended(val line: Line, override val until: Instant? = null) : PushMessage

    data class Planned(val count: Int, val titles: String) : PushMessage

    /** The backend has not managed to check for a while: no notification no longer means no disruption. */
    data object Outage : PushMessage

    /** Checks work again, after an outage. */
    data object Recovered : PushMessage

    /** When the watch window ends: a line's message goes away then. Null for the others. */
    val until: Instant?
        get() = null

    /** Notifications with the same tag replace each other. */
    val tag: String
        get() = when (this) {
            is Started -> "line-${line.name}"
            is Changed -> "line-${line.name}"
            is Ended -> "line-${line.name}"
            is Planned -> "planned"
            Outage, Recovered -> "watch"
        }

    companion object {
        /** Null when the data is not a message this version knows, e.g. from a newer backend. */
        fun parse(data: Map<String, String>): PushMessage? {
            val line = data["line"]?.let { name -> Line(name, mode(data["mode"])) }
            // Unreadable, it is left out: better shown too long than not at all.
            val until = data["until"]?.let { runCatching { Instant.parse(it) }.getOrNull() }
            return when (data["type"]) {
                "started" -> Started(
                    line ?: return null,
                    severity(data["severity"]) ?: return null,
                    data["title"],
                    data["text"],
                    until,
                )
                "changed" -> Changed(
                    line ?: return null,
                    severity(data["severity"]) ?: return null,
                    severity(data["previous"]) ?: return null,
                    data["title"],
                    data["text"],
                    until,
                )
                "ended" -> Ended(line ?: return null, until)
                "planned" -> Planned(
                    data["count"]?.toIntOrNull() ?: return null,
                    data["titles"] ?: return null,
                )
                "outage" -> Outage
                "recovered" -> Recovered
                else -> null
            }
        }

        /** `train`, `metro`, or any other mode, as named by the backend. */
        fun mode(name: String?) = when (name) {
            "train" -> Mode.TRAIN
            "metro" -> Mode.METRO
            else -> Mode.OTHER
        }

        private fun severity(name: String?) = when (name) {
            "blocking" -> Severity.BLOCKING
            "disrupted" -> Severity.DISRUPTED
            else -> null
        }
    }
}
