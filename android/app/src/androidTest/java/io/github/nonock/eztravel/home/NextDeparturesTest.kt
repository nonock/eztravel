package io.github.nonock.eztravel.home

import io.github.nonock.eztravel.ScreenTesting
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.Density
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.filter
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Departures
import io.github.nonock.eztravel.backend.Departures.Departure
import io.github.nonock.eztravel.backend.Departures.Leg
import io.github.nonock.eztravel.backend.Departures.State
import io.github.nonock.eztravel.backend.Position
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.backend.Refusal
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.backend.Status.LineState
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import java.time.Duration
import java.time.Instant

@RunWith(AndroidJUnit4::class)
class NextDeparturesTest : ScreenTesting() {
    private fun at(time: String) = Instant.parse("2026-11-03T${time}Z")

    private val now = at("06:50:00")

    /** In a column that scrolls, as on the Trajet tab. */
    private fun show(backend: FakeBackend) = compose.setContent {
        Column(Modifier.verticalScroll(rememberScrollState())) { NextDepartures(backend, now = { now }) }
    }

    /** Below the fold: scrolled to, once read. */
    private fun scrolledTo(matcher: SemanticsMatcher) {
        compose.waitUntil(timeoutMillis = 5_000) {
            compose.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNode(matcher).performScrollTo().assertIsDisplayed()
    }

    /** A leg's header: from one stop to the other. */
    private fun leg(from: String, to: String) = hasText(from) and hasContentDescription(text(R.string.departures_leg_to)) and hasText(to)

    private val train = Line("L", Mode.TRAIN)
    private val metro = Line("X", Mode.METRO)

    private val morning = Departures(
        Direction.OUTBOUND,
        listOf(
            Leg(
                train,
                0xFF7B61C9.toInt(),
                "Les Aubépines",
                "Gare Centrale",
                listOf(
                    Departure(at("06:52:00"), at("06:52:00"), at("07:04:00"), "PALO", "Gare Centrale", "2", State.ON_TIME, "j1"),
                    Departure(at("07:02:00"), at("07:02:00"), at("07:14:00"), "PILO", "Gare Centrale", "2", State.ON_TIME, "j2"),
                    Departure(at("07:07:00"), at("07:11:00"), at("07:23:00"), "PILO", "Gare Centrale", "2", State.DELAYED, "j3"),
                    Departure(at("07:22:00"), at("07:22:00"), at("07:34:00"), "PALO", "Gare Centrale", "2", State.CANCELLED, "j4"),
                    // Said late, with no time yet.
                    Departure(at("07:37:00"), at("07:37:00"), null, null, "Les Tilleuls", null, State.DELAYED, "j5"),
                ),
                reach = Duration.ofMinutes(12),
            ),
            Leg(
                metro,
                0xFFE07A1F.toInt(),
                "Gare Centrale",
                "Mairie",
                listOf(
                    Departure(null, at("06:50:20"), null, null, "Porte Nord", null, State.ON_TIME, "m1"),
                    Departure(null, at("06:53:30"), null, null, "Porte Nord", null, State.ON_TIME, "m2"),
                ),
            ),
        ),
    )

    @Test
    fun eachLegShowsItsNextDepartures() {
        show(FakeBackend(departures = morning))
        waitFor(text(R.string.departures_title))
        waitFor(hasContentDescription(text(R.string.line_train, "L")) and leg("Les Aubépines", "Gare Centrale"))
        // On time: its time and its arrival, read "arrives 07:04", where it goes, and its platform below the time.
        waitFor(
            hasText(at("06:52:00").clock()) and hasText("Gare Centrale") and
                hasText(text(R.string.departures_platform, "2")) and hasContentDescription(text(R.string.departures_arrival, at("07:04:00").clock())),
        )
        // The operator's codes say nothing to most; a platform shows as soon as it is known.
        compose.onNodeWithText("PALO").assertDoesNotExist()
        waitFor(hasText(at("07:22:00").clock()) and hasText(text(R.string.departures_platform, "2")))
        // Late: the timetable's time and the delay, as on station boards.
        waitFor(hasText(at("07:07:00").clock()) and hasText(text(R.string.departures_late, 4)))
        waitFor(hasText(at("07:22:00").clock()) and hasText(text(R.string.departures_cancelled)))
        // The metro's next trains on one line: "Arriving · 3 min, Porte Nord".
        waitFor(hasText(at("07:37:00").clock()) and hasText(text(R.string.departures_delayed)) and hasText("Les Tilleuls"))
        // The metro gives minutes left.
        waitFor(hasContentDescription(text(R.string.line_metro, "X")) and leg("Gare Centrale", "Mairie"))
        waitFor(hasText(text(R.string.departures_arriving)) and hasText("Porte Nord"))
        waitFor(hasText(text(R.string.departures_minutes, 3)) and hasText("Porte Nord"))
    }

    @Test
    fun aLegWithItsTimeToThePlatformSaysWhenToLeave() {
        show(FakeBackend(departures = morning))
        // Twelve minutes to the platform, at 6:50. Too soon to leave for the 6:52:
        // when it leaves, for whoever is already on the way.
        waitFor(hasText(at("06:52:00").clock()) and hasText(text(R.string.departures_leaves_in, 2)))
        compose.onNode(hasText(at("06:52:00").clock()) and hasText(text(R.string.departures_leave_in, 0).substringBefore("0"), substring = true))
            .assertDoesNotExist()
        waitFor(hasText(at("07:02:00").clock()) and hasText(text(R.string.departures_leave_now)))
        waitFor(hasText(at("07:07:00").clock()) and hasText(text(R.string.departures_leave_in, 9)))
        waitFor(hasText(at("07:37:00").clock()) and hasText(text(R.string.departures_leave_in, 35)))
        // Nothing to leave for: a cancelled train, or the metro without a time to its platform.
        compose.onNode(hasText(at("07:22:00").clock()) and hasText(text(R.string.departures_leave_in, 20))).assertDoesNotExist()
        compose.onAllNodes(hasText(text(R.string.departures_leave_in, 3).substringBefore("3"), substring = true))
            .filter(hasText("Porte Nord"))
            .assertCountEquals(0)
    }

    /** The bell that picks the train leaving at [aimed], or drops it once picked. */
    private fun bell(aimed: String) = hasContentDescription(text(R.string.departures_pick, at(aimed).clock()))

    private fun picked(aimed: String) = hasContentDescription(text(R.string.departures_unpick, at(aimed).clock()))

    /** Instead of the metro, interrupted: metro Y, then train Z, disrupted too. */
    private val withBackup = morning.copy(
        backups = listOf(
            Departures.Backup(
                listOf(LineState(metro, Severity.BLOCKING, 0xFFE07A1F.toInt())),
                listOf(
                    Leg(
                        Line("Y", Mode.METRO),
                        0xFF3E9C6B.toInt(),
                        "Gare Centrale",
                        "Le Port",
                        listOf(Departure(null, at("06:54:00"), null, null, "Porte Sud", null, State.ON_TIME, "y1")),
                    ),
                    Leg(
                        Line("Z", Mode.TRAIN),
                        null,
                        "Le Port",
                        "Mairie",
                        listOf(Departure(at("07:05:00"), at("07:05:00"), at("07:12:00"), "ZOLA", "Mairie", "1", State.ON_TIME, "z1")),
                        severity = Severity.DISRUPTED,
                    ),
                ),
            ),
        ),
    )

    @Test
    fun aBackupShowsItsRidesInsteadOfTheLineDisrupted() {
        show(FakeBackend(departures = withBackup))
        scrolledTo(hasText(text(R.string.backup_title)))
        // The line it stands in for, read as one: "Metro X, interrupted".
        scrolledTo(hasContentDescription(text(R.string.line_metro, "X")) and hasText(text(R.string.status_state_blocking)))
        scrolledTo(hasContentDescription(text(R.string.line_metro, "Y")) and leg("Gare Centrale", "Le Port"))
        scrolledTo(hasText(text(R.string.departures_minutes, 4)) and hasText("Porte Sud"))
        // A ride disrupted too says so, with its line.
        scrolledTo(hasContentDescription(text(R.string.line_train, "Z")) and leg("Le Port", "Mairie") and hasText(text(R.string.status_state_disrupted)))
        compose.onNode(hasContentDescription(text(R.string.line_metro, "Y")) and hasText(text(R.string.status_state_disrupted)))
            .assertDoesNotExist()
        scrolledTo(hasText(at("07:05:00").clock()) and hasContentDescription(text(R.string.departures_arrival, at("07:12:00").clock())))
        // Never picked: the reminder is for the way's own train.
        compose.onNode(bell("07:05:00")).assertDoesNotExist()
        scrolledTo(bell("07:07:00"))
    }

    @Test
    fun noBackupWhileTheWayRuns() {
        show(FakeBackend(departures = morning))
        waitFor(leg("Gare Centrale", "Mairie"))
        compose.onNodeWithText(text(R.string.backup_title)).assertDoesNotExist()
    }

    @Test
    fun aTrainIsPickedForTheReminderAndDropped() {
        val backend = FakeBackend(departures = morning)
        show(backend)
        waitFor(bell("07:07:00"))
        compose.onNode(bell("07:07:00")).performClick()
        waitFor(hasText(at("07:07:00").clock()) and hasText(text(R.string.departures_picked)))
        compose.onNode(picked("07:07:00")).assertIsDisplayed()
        assertEquals(listOf("j3"), backend.picks)
        // Another one in its place.
        compose.onNode(bell("07:37:00")).performClick()
        waitFor(hasText(at("07:37:00").clock()) and hasText(text(R.string.departures_picked)))
        compose.onAllNodes(hasText(text(R.string.departures_picked))).assertCountEquals(1)
        compose.onNode(bell("07:07:00")).assertIsDisplayed()
        // Its bell again, and it is dropped.
        compose.onNode(picked("07:37:00")).performClick()
        compose.waitUntil(timeoutMillis = 5_000) { backend.picks.size == 3 }
        assertEquals(listOf("j3", "j5", null), backend.picks)
        compose.waitUntil(timeoutMillis = 5_000) {
            compose.onAllNodes(hasText(text(R.string.departures_picked))).fetchSemanticsNodes().isEmpty()
        }
    }

    @Test
    fun theRideNearestThePhoneComesFirst() {
        val placed = morning.copy(
            legs = listOf(
                morning.legs[0].copy(fromPosition = Position(48.947, 2.248)),
                morning.legs[1].copy(fromPosition = Position(48.875, 2.326)),
            ),
        )
        compose.setContent {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                NextDepartures(FakeBackend(departures = placed), now = { now }, where = { Position(48.8752, 2.3262) })
            }
        }
        // At Gare Centrale: the metro above the train.
        waitFor(leg("Gare Centrale", "Mairie"))
        val top = { matcher: SemanticsMatcher -> compose.onNode(matcher).fetchSemanticsNode().boundsInRoot.top }
        compose.waitUntil(timeoutMillis = 5_000) {
            top(leg("Gare Centrale", "Mairie")) < top(leg("Les Aubépines", "Gare Centrale"))
        }
    }

    @Test
    fun aLongListShowsTheTrainsToCatchFirstAndTheRestWhenAsked() {
        // Every six minutes from 6:56: with twelve minutes to the platform, at 6:50
        // the 6:56 is too soon, and the 7:02, 7:08 and 7:14 can be caught.
        val many = (0 until 9).map { index ->
            val time = at("06:56:00").plus(Duration.ofMinutes(6L * index))
            Departure(time, time, null, null, "Gare Centrale", null, State.ON_TIME, "t$index")
        }
        show(FakeBackend(departures = morning.copy(legs = listOf(morning.legs[0].copy(departures = many)))))
        scrolledTo(hasText(at("06:56:00").clock()) and hasText(text(R.string.departures_leaves_in, 6)))
        scrolledTo(hasText(at("07:14:00").clock()))
        compose.onNode(hasText(at("07:20:00").clock())).assertDoesNotExist()
        compose.onNodeWithText(context.resources.getQuantityString(R.plurals.departures_more, 5, 5)).performScrollTo().performClick()
        scrolledTo(hasText(at("07:44:00").clock()) and hasText(text(R.string.departures_leave_in, 42)))
        compose.onNodeWithText(text(R.string.departures_less)).performScrollTo().performClick()
        compose.waitUntil(timeoutMillis = 5_000) {
            compose.onAllNodes(hasText(at("07:44:00").clock())).fetchSemanticsNodes().isEmpty()
        }
    }

    @Test
    fun aMetroShowsItsNextTrainsOnALinePerDestination() {
        val trains = listOf(
            Departure(null, at("06:51:10"), null, null, "Porte Nord", null, State.ON_TIME, "m1"),
            Departure(null, at("06:56:00"), null, null, "Porte Nord", null, State.ON_TIME, "m2"),
            Departure(null, at("06:59:30"), null, null, "La Fourche", null, State.ON_TIME, "m3"),
            Departure(null, at("07:03:00"), null, null, "Porte Nord", null, State.ON_TIME, "m4"),
        )
        show(FakeBackend(departures = Departures(Direction.OUTBOUND, listOf(Leg(metro, null, "Gare Centrale", "Mairie", trains)))))
        // "1 · 6 · 13 min, Porte Nord", and the short run on its own: "9 min, La Fourche".
        waitFor(hasText("1") and hasText("6") and hasText(text(R.string.departures_minutes, 13)) and hasText("Porte Nord"))
        waitFor(hasText(text(R.string.departures_minutes, 9)) and hasText("La Fourche"))
    }

    @Test
    fun onlyATrainThatCanStillBeCaughtCanBePicked() {
        show(FakeBackend(departures = morning))
        waitFor(bell("07:02:00"))
        // Too late, cancelled, or the metro, without a time to its platform: three bells only.
        compose.onNode(bell("06:52:00")).assertDoesNotExist()
        compose.onNode(bell("07:22:00")).assertDoesNotExist()
        compose.onAllNodes(hasClickAction()).assertCountEquals(3)
    }

    @Test
    fun anOlderBackendOffersNoPick() {
        val older = morning.copy(
            legs = morning.legs.map { leg -> leg.copy(departures = leg.departures?.map { it.copy(journey = null) }) },
        )
        show(FakeBackend(departures = older))
        waitFor(hasText(at("07:07:00").clock()))
        compose.onAllNodes(hasClickAction()).assertCountEquals(0)
    }

    @Test
    fun aRefusedPickSaysWhy() {
        val backend = FakeBackend(departures = morning)
        show(backend)
        waitFor(bell("07:07:00"))
        val reasons = mapOf(
            Refusal.TOO_LATE to R.string.departures_refused_too_late,
            Refusal.NOT_OFFICE_DAY to R.string.departures_refused_not_office_day,
            Refusal.OUTSIDE_WINDOW to R.string.departures_refused_outside_window,
            Refusal.UNKNOWN to R.string.departures_refused_unknown,
            Refusal.UNAVAILABLE to R.string.departures_refused_unavailable,
        )
        for ((refusal, reason) in reasons) {
            backend.refusal = refusal
            compose.onNode(bell("07:07:00")).performClick()
            waitFor(text(reason))
        }
        compose.onNode(hasText(text(R.string.departures_picked))).assertDoesNotExist()
        // Saved after all: the reason goes.
        backend.refusal = null
        compose.onNode(bell("07:07:00")).performClick()
        waitFor(hasText(at("07:07:00").clock()) and hasText(text(R.string.departures_picked)))
        compose.onNodeWithText(text(R.string.departures_refused_unavailable)).assertDoesNotExist()
    }

    @Test
    fun aPickThatCannotBeSavedSaysSo() {
        show(FakeBackend(departures = morning, pickFailing = true))
        waitFor(bell("07:07:00"))
        compose.onNode(bell("07:07:00")).performClick()
        waitFor(text(R.string.departures_pick_failed))
        compose.onNode(hasText(text(R.string.departures_picked))).assertDoesNotExist()
    }

    /** The card on top, about the train shown first: read as one. */
    private fun hero(label: Int) = hasText(text(label))

    @Test
    fun theNextTrainToCatchComesFirstInBig() {
        show(FakeBackend(departures = morning))
        // At 7:50, the 7:52 is gone for whoever is not at the platform: the 8:02 it is, now.
        waitFor(hero(R.string.departures_hero_next) and hasText(at("07:02:00").clock()) and hasText(text(R.string.departures_hero_now)))
        compose.onNode(hero(R.string.departures_hero_picked)).assertDoesNotExist()
    }

    @Test
    fun thePickedTrainComesFirstWithItsCountdown() {
        val picked = morning.picking("j3")
        show(FakeBackend(departures = picked))
        // The 8:07, 4 minutes late, leaves at 8:11: leave at 7:59, in 9 minutes.
        waitFor(
            hero(R.string.departures_hero_picked) and hasText(at("07:07:00").clock()) and hasText(text(R.string.departures_late, 4)) and
                hasText(text(R.string.departures_hero_leave_in)) and hasText(text(R.string.departures_minutes, 9)) and
                hasText("Gare Centrale") and hasText(text(R.string.departures_platform, "2")),
        )
    }

    @Test
    fun theCountdownMovesBetweenReads() {
        var clock = at("06:50:00")
        compose.setContent { NextDepartures(FakeBackend(departures = morning.picking("j3")), now = { clock }, tickMs = 50) }
        waitFor(hero(R.string.departures_hero_picked) and hasText(text(R.string.departures_minutes, 9)))
        clock = at("06:54:00")
        waitFor(hero(R.string.departures_hero_picked) and hasText(text(R.string.departures_minutes, 5)))
    }

    @Test
    fun aPickedTrainPastItsTimeToLeaveSaysWhenItLeaves() {
        // On the way to it, most likely.
        var clock = at("06:50:00")
        compose.setContent { NextDepartures(FakeBackend(departures = morning.picking("j1")), now = { clock }, tickMs = 50) }
        waitFor(hero(R.string.departures_hero_picked) and hasText(at("06:52:00").clock()) and hasText(text(R.string.departures_leaves_in, 2)))
        clock = at("06:52:10")
        waitFor(hero(R.string.departures_hero_picked) and hasText(text(R.string.departures_leaving)))
        waitFor(hasText(at("06:52:00").clock()) and hasContentDescription(text(R.string.departures_arrival, at("07:04:00").clock())) and hasText(text(R.string.departures_leaving)))
    }

    @Test
    fun withoutAPickTheUsualTrainComesFirst() {
        val trains = morning.legs[0].departures!!.map { if (it.journey == "j3") it.copy(usual = true) else it }
        show(FakeBackend(departures = morning.copy(legs = listOf(morning.legs[0].copy(departures = trains), morning.legs[1]))))
        // The 8:07, rather than the 8:02, the next one to catch.
        waitFor(hero(R.string.departures_hero_usual) and hasText(at("07:07:00").clock()))
        waitFor(hasText(at("07:07:00").clock()) and hasContentDescription(text(R.string.departures_arrival, at("07:23:00").clock())) and hasText(text(R.string.departures_usual)))
        compose.onNode(hero(R.string.departures_hero_next)).assertDoesNotExist()
    }

    @Test
    fun atTwiceTheFontSizeACountdownStaysOnOneLine() {
        // Late, from its platform, the usual train: a row full of details.
        val trains = morning.legs[0].departures!!.map { if (it.journey == "j3") it.copy(usual = true) else it }
        val backend = FakeBackend(departures = morning.copy(legs = listOf(morning.legs[0].copy(departures = trains), morning.legs[1])))
        compose.setContent {
            CompositionLocalProvider(LocalDensity provides Density(LocalDensity.current.density, fontScale = 2f)) {
                Column(Modifier.verticalScroll(rememberScrollState())) { NextDepartures(backend, now = { now }) }
            }
        }
        val countdown = hasText(text(R.string.departures_leave_in, 9))
        compose.waitUntil(timeoutMillis = 5_000) {
            compose.onAllNodes(countdown, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()
        }
        for (node in compose.onAllNodes(countdown, useUnmergedTree = true).fetchSemanticsNodes()) {
            val layouts = mutableListOf<TextLayoutResult>()
            node.config[SemanticsActions.GetTextLayoutResult].action!!.invoke(layouts)
            assertEquals("lines of \"${layouts.single().layoutInput.text}\"", 1, layouts.single().lineCount)
        }
    }

    @Test
    fun noTrainToCatchLeavesTheCardOut() {
        val metroOnly = morning.copy(legs = morning.legs.drop(1))
        show(FakeBackend(departures = metroOnly))
        waitFor(leg("Gare Centrale", "Mairie"))
        compose.onNode(hero(R.string.departures_hero_next)).assertDoesNotExist()
    }

    @Test
    fun onlyTheWayHomeCanBeSetLater() {
        val backend = FakeBackend(departures = morning)
        val refresh = androidx.compose.runtime.mutableIntStateOf(0)
        val late = io.github.nonock.eztravel.backend.Status.Late(java.time.LocalTime.of(22, 0), java.time.LocalTime.of(23, 0), set = true)
        compose.setContent {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                NextDepartures(backend, now = { now }, refresh = refresh.intValue, late = late)
            }
        }
        waitFor(leg("Gare Centrale", "Mairie"))
        compose.onNode(hasText(text(R.string.late_change))).assertDoesNotExist()
        // In the evening, the way home.
        backend.departures = morning.copy(direction = Direction.INBOUND)
        compose.runOnIdle { refresh.intValue++ }
        waitFor(text(R.string.late_change))
    }

    @Test
    fun aLegWithoutDeparturesSaysWhy() {
        val evening = Departures(
            Direction.INBOUND,
            listOf(
                Leg(metro, null, "Mairie", "Gare Centrale", departures = null),
                Leg(train, null, "Gare Centrale", "Les Aubépines", emptyList()),
                // Its stops are not known yet.
                Leg(Line("Y", Mode.METRO), null, from = null, to = null, departures = null),
            ),
        )
        show(FakeBackend(departures = evening))
        waitFor(leg("Mairie", "Gare Centrale"))
        waitFor(text(R.string.departures_none))
        waitFor(hasContentDescription(text(R.string.line_metro, "Y")))
        compose.waitUntil(timeoutMillis = 5_000) {
            compose.onAllNodes(hasText(text(R.string.departures_unavailable))).fetchSemanticsNodes().size == 2
        }
    }

    @Test
    fun anUnreachableServerLeavesTheDeparturesUnknown() {
        show(FakeBackend(failing = true))
        waitFor(text(R.string.departures_unavailable))
    }

    @Test
    fun aGarbledAnswerIsTakenAsUnknownDepartures() {
        show(FakeBackend(garbled = true))
        waitFor(text(R.string.departures_unavailable))
    }

    @Test
    fun aBuildWithoutServerShowsNoDepartures() {
        show(FakeBackend(configured = false, departures = morning))
        compose.waitForIdle()
        compose.onNodeWithText(text(R.string.departures_title)).assertDoesNotExist()
    }
}
