package io.github.nonock.eztravel.notifications

import java.time.Instant
import java.time.LocalDate

/**
 * A push message from the backend. Its format is fixed by `fixtures/push/` in
 * the repository, which the backend tests read too.
 */
sealed interface PushMessage {
    data class Line(val name: String, val mode: Mode)

    enum class Mode { TRAIN, METRO, OTHER }

    enum class Severity { BLOCKING, DISRUPTED }

    /** Where a line's disruption is, as the backend says. */
    sealed interface Scope {
        /** The whole line, as far as the operator says. */
        data object WholeLine : Scope

        data class Section(val from: String, val to: String) : Scope

        /** Its stops: the first, and how many. */
        data class Stops(val first: String, val count: Int) : Scope
    }

    /**
     * `title` and `text` are the operator's own words, in French, as published;
     * `id` is the record's, and `scope` where it is, from a backend that says.
     */
    data class Started(
        val line: Line,
        val severity: Severity,
        val title: String?,
        val text: String?,
        override val until: Instant? = null,
        val id: String? = null,
        val scope: Scope? = null,
    ) : PushMessage

    data class Changed(
        val line: Line,
        val severity: Severity,
        val previous: Severity,
        val title: String?,
        val text: String?,
        override val until: Instant? = null,
        val id: String? = null,
        val scope: Scope? = null,
    ) : PushMessage

    data class Ended(val line: Line, override val until: Instant? = null) : PushMessage

    data class Planned(val count: Int, val titles: String) : PushMessage

    /** The evening before an office day, [date]: what will disrupt the way, [count] records by their [titles]. */
    data class Tomorrow(val date: LocalDate, val count: Int, val titles: String, override val until: Instant?) : PushMessage

    /** The backend has not managed to check for a while: no notification no longer means no disruption. */
    data object Outage : PushMessage

    /** Checks work again, after an outage. */
    data object Recovered : PushMessage

    /**
     * A train by its timetable departure from the stop it leaves; `mission` is
     * its code, when known, and `expected` its departure as last listed.
     */
    data class Train(
        val line: Line,
        val mission: String?,
        val destination: String,
        val from: String,
        val aimed: Instant,
        val expected: Instant? = null,
    )

    /** How the backend knows a train is cancelled. */
    enum class Reason {
        /** It left its station's list before its time, and stayed out. */
        MISSING,

        /** The operator says so. */
        ANNOUNCED,
    }

    /** A train of the window will not run. */
    data class TrainCancelled(val train: Train, val reason: Reason, override val until: Instant?) : PushMessage

    /** A train notified as cancelled runs after all. */
    data class TrainBack(val train: Train, override val until: Instant?) : PushMessage

    /**
     * Time to [leave] for [train]. [live] when PRIM follows the train live: a
     * train that has not set off yet is only still scheduled.
     */
    data class Leave(
        val train: Train,
        val leave: Instant,
        val platform: String?,
        val live: Boolean,
        override val until: Instant?,
    ) : PushMessage

    /** The train of the reminder now runs late, to leave at its `expected` time. */
    data class TrainLate(val train: Train, override val until: Instant?) : PushMessage

    /** When the watch window ends, or the train's time: the message goes away then. Null for the others. */
    val until: Instant?
        get() = null

    /** What tapping it opens in the app: a line's alerts, or the page alone; null for the commute. */
    val opening: Opening?
        get() = when (this) {
            is Started -> Opening(line.name, id, title)
            is Changed -> Opening(line.name, id, title)
            is Ended -> Opening(line.name, null, null)
            // Planned works may be on several lines.
            is Planned, is Tomorrow -> Opening(null, null, null)
            else -> null
        }

    /** Notifications with the same tag replace each other. */
    val tag: String
        get() = when (this) {
            is Started -> "line-${line.name}"
            is Changed -> "line-${line.name}"
            is Ended -> "line-${line.name}"
            is Planned -> "planned"
            is Tomorrow -> "tomorrow"
            Outage, Recovered -> "watch"
            // A train's own: neither its line's state nor another train.
            is TrainCancelled -> "train-${train.line.name}-${train.aimed}"
            is TrainBack -> "train-${train.line.name}-${train.aimed}"
            is Leave -> "train-${train.line.name}-${train.aimed}"
            is TrainLate -> "train-${train.line.name}-${train.aimed}"
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
                    data["id"],
                    scope(data),
                )
                "changed" -> Changed(
                    line ?: return null,
                    severity(data["severity"]) ?: return null,
                    severity(data["previous"]) ?: return null,
                    data["title"],
                    data["text"],
                    until,
                    data["id"],
                    scope(data),
                )
                "ended" -> Ended(line ?: return null, until)
                "planned" -> Planned(
                    data["count"]?.toIntOrNull() ?: return null,
                    data["titles"] ?: return null,
                )
                "tomorrow" -> Tomorrow(
                    data["date"]?.let { runCatching { LocalDate.parse(it) }.getOrNull() } ?: return null,
                    data["count"]?.toIntOrNull() ?: return null,
                    data["titles"] ?: return null,
                    until,
                )
                "outage" -> Outage
                "recovered" -> Recovered
                "train_cancelled" -> TrainCancelled(
                    train(data, line) ?: return null,
                    // Whatever a newer backend's reason, the train is still cancelled.
                    if (data["reason"] == "announced") Reason.ANNOUNCED else Reason.MISSING,
                    until,
                )
                "train_back" -> TrainBack(train(data, line) ?: return null, until)
                "leave" -> Leave(
                    train(data, line) ?: return null,
                    instant(data["leave"]) ?: return null,
                    data["platform"],
                    data["live"] == "true",
                    until,
                )
                "train_late" -> TrainLate(
                    train(data, line)?.takeIf { it.expected != null } ?: return null,
                    until,
                )
                else -> null
            }
        }

        private fun train(data: Map<String, String>, line: Line?): Train? {
            return Train(
                line ?: return null,
                data["mission"],
                data["destination"] ?: return null,
                data["station"] ?: return null,
                instant(data["aimed"]) ?: return null,
                instant(data["expected"]),
            )
        }

        /** Null from a backend that does not say where, or says it in a way this version cannot read. */
        private fun scope(data: Map<String, String>): Scope? = when (data["scope"]) {
            "line" -> Scope.WholeLine
            "section" -> Scope.Section(data["section_from"] ?: return null, data["section_to"] ?: return null)
            "stops" -> Scope.Stops(data["stop"] ?: return null, data["stop_count"]?.toIntOrNull() ?: 1)
            else -> null
        }

        private fun instant(text: String?): Instant? = text?.let { runCatching { Instant.parse(it) }.getOrNull() }

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

/**
 * A place on the Alerts page, opened from a notification: [line]'s alerts,
 * its record unfolded, by its [record] id or else its [title]; without a
 * line, the page alone.
 */
data class Opening(val line: String?, val record: String?, val title: String?)
