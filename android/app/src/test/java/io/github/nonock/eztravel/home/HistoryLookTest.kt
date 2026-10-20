package io.github.nonock.eztravel.home

import io.github.nonock.eztravel.backend.HistoryEntry
import io.github.nonock.eztravel.notifications.PushMessage
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import org.junit.Assert.assertEquals
import org.junit.Test
import java.time.Instant

class HistoryLookTest {
    private val at = Instant.parse("2026-11-03T06:42:00Z")
    private val l = Line("L", Mode.TRAIN)
    private val train = PushMessage.Train(l, "PALO", "Gare Centrale", "Les Aubépines", at, at)

    private fun sent(message: PushMessage) = HistoryEntry.Sent(at, message).look()

    @Test
    fun aLineMessageShowsItsLineToldByHowBadItIs() {
        assertEquals(Look(Mark.Line(l), Tone.BAD), sent(PushMessage.Started(l, Severity.BLOCKING, null, null)))
        assertEquals(Look(Mark.Line(l), Tone.WARN), sent(PushMessage.Started(l, Severity.DISRUPTED, null, null)))
        assertEquals(Look(Mark.Line(l), Tone.BAD), sent(PushMessage.Changed(l, Severity.BLOCKING, Severity.DISRUPTED, null, null)))
        assertEquals(Look(Mark.Line(l), Tone.WARN), sent(PushMessage.Changed(l, Severity.DISRUPTED, Severity.BLOCKING, null, null)))
        assertEquals(Look(Mark.Line(l), Tone.GOOD), sent(PushMessage.Ended(l)))
        assertEquals(Look(Mark.Line(l), Tone.BAD), HistoryEntry.WindowClosed(at, l, Severity.BLOCKING).look())
        assertEquals(Look(Mark.Line(l), Tone.WARN), HistoryEntry.WindowClosed(at, l, Severity.DISRUPTED).look())
    }

    @Test
    fun aTrainShowsItsLineAndTheReminderItsBell() {
        assertEquals(Look(Mark.Line(l), Tone.BAD), sent(PushMessage.TrainCancelled(train, PushMessage.Reason.MISSING, null)))
        assertEquals(Look(Mark.Line(l), Tone.GOOD), sent(PushMessage.TrainBack(train, null)))
        assertEquals(Look(Mark.Line(l), Tone.WARN), sent(PushMessage.TrainLate(train, null)))
        assertEquals(Look(Mark.Reminder, Tone.INFO), sent(PushMessage.Leave(train, at, "2", live = true, until = null)))
    }

    @Test
    fun theWatchAndThePlannedWorksHaveTheirOwnMarks() {
        assertEquals(Look(Mark.WatchDown, Tone.BAD), sent(PushMessage.Outage))
        assertEquals(Look(Mark.WatchUp, Tone.GOOD), sent(PushMessage.Recovered))
        assertEquals(Look(Mark.Planned, Tone.INFO), sent(PushMessage.Planned(2, "Ligne L : travaux")))
    }
}
