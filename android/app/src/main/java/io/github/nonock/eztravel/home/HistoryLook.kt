package io.github.nonock.eztravel.home

import io.github.nonock.eztravel.backend.HistoryEntry
import io.github.nonock.eztravel.notifications.PushMessage

/** What an entry of the history is about, shown before its words. */
sealed interface Mark {
    /** A line's state, or one of its trains: the line's badge. */
    data class Line(val line: PushMessage.Line) : Mark

    /** The reminder to leave: a bell. */
    data object Reminder : Mark

    /** Checks stopped, or work again. */
    data object WatchDown : Mark
    data object WatchUp : Mark

    /** The day's planned works. */
    data object Planned : Mark
}

/** How it went: told by the rail along the entry. */
enum class Tone { BAD, WARN, GOOD, INFO }

data class Look(val mark: Mark, val tone: Tone)

/** How an entry reads at a glance. */
fun HistoryEntry.look(): Look = when (this) {
    is HistoryEntry.WindowClosed -> Look(Mark.Line(line), severity.tone())
    is HistoryEntry.Sent -> when (val message = message) {
        is PushMessage.Started -> Look(Mark.Line(message.line), message.severity.tone())
        is PushMessage.Changed -> Look(Mark.Line(message.line), message.severity.tone())
        is PushMessage.Ended -> Look(Mark.Line(message.line), Tone.GOOD)
        is PushMessage.TrainCancelled -> Look(Mark.Line(message.train.line), Tone.BAD)
        is PushMessage.TrainBack -> Look(Mark.Line(message.train.line), Tone.GOOD)
        is PushMessage.TrainLate -> Look(Mark.Line(message.train.line), Tone.WARN)
        is PushMessage.Leave -> Look(Mark.Reminder, Tone.INFO)
        PushMessage.Outage -> Look(Mark.WatchDown, Tone.BAD)
        PushMessage.Recovered -> Look(Mark.WatchUp, Tone.GOOD)
        is PushMessage.Planned -> Look(Mark.Planned, Tone.INFO)
        // It may change the day.
        is PushMessage.Tomorrow -> Look(Mark.Planned, Tone.WARN)
    }
}

private fun PushMessage.Severity.tone() = if (this == PushMessage.Severity.BLOCKING) Tone.BAD else Tone.WARN
