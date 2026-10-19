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

    /** A train by its timetable departure from the stop it leaves; `mission` is its code, when known. */
    data class Train(val line: Line, val mission: String?, val destination: String, val from: String, val aimed: Instant)

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

    /** When the watch window ends, or the train's time: the message goes away then. Null for the others. */
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
            // A train's own: neither its line's state nor another train.
            is TrainCancelled -> "train-${train.line.name}-${train.aimed}"
            is TrainBack -> "train-${train.line.name}-${train.aimed}"
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
                "train_cancelled" -> TrainCancelled(
                    train(data, line) ?: return null,
                    // Whatever a newer backend's reason, the train is still cancelled.
                    if (data["reason"] == "announced") Reason.ANNOUNCED else Reason.MISSING,
                    until,
                )
                "train_back" -> TrainBack(train(data, line) ?: return null, until)
                else -> null
            }
        }

        private fun train(data: Map<String, String>, line: Line?): Train? {
            return Train(
                line ?: return null,
                data["mission"],
                data["destination"] ?: return null,
                data["from"] ?: return null,
                data["aimed"]?.let { runCatching { Instant.parse(it) }.getOrNull() } ?: return null,
            )
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
