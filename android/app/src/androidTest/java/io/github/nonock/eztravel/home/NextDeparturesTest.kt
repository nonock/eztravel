package io.github.nonock.eztravel.home

import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.filter
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Departures
import io.github.nonock.eztravel.backend.Departures.Departure
import io.github.nonock.eztravel.backend.Departures.Leg
import io.github.nonock.eztravel.backend.Departures.State
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.backend.Refusal
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.time.Duration
import java.time.Instant

@RunWith(AndroidJUnit4::class)
class NextDeparturesTest {
    @get:Rule
    val compose = createComposeRule()

    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    private fun text(id: Int, vararg args: Any) = context.getString(id, *args)

    private fun at(time: String) = Instant.parse("2026-11-03T${time}Z")

    private val now = at("06:50:00")

    private fun show(backend: FakeBackend) = compose.setContent { NextDepartures(backend, now = { now }) }

    /** The departures are read once the screen is resumed: wait for them. */
    private fun waitFor(matcher: SemanticsMatcher) {
        compose.waitUntil(timeoutMillis = 5_000) {
            compose.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNode(matcher).assertIsDisplayed()
    }

    private fun waitFor(text: String) = waitFor(hasText(text))

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
        // On time: its time, where it goes, its mission, platform and arrival.
        waitFor(
            hasText(at("06:52:00").clock()) and hasText("Gare Centrale") and hasText("PALO") and
                hasText(text(R.string.departures_platform, "2")) and hasText(text(R.string.departures_arrival, at("07:04:00").clock())),
        )
        // Late: the timetable's time and the delay, as on station boards.
        waitFor(hasText(at("07:07:00").clock()) and hasText(text(R.string.departures_late, 4)))
        waitFor(hasText(at("07:22:00").clock()) and hasText(text(R.string.departures_cancelled)))
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
        waitFor(hasText(at("06:52:00").clock()) and hasText("PALO") and hasText(text(R.string.departures_leaving)))
    }

    @Test
    fun withoutAPickTheUsualTrainComesFirst() {
        val trains = morning.legs[0].departures!!.map { if (it.journey == "j3") it.copy(usual = true) else it }
        show(FakeBackend(departures = morning.copy(legs = listOf(morning.legs[0].copy(departures = trains), morning.legs[1]))))
        // The 8:07, rather than the 8:02, the next one to catch.
        waitFor(hero(R.string.departures_hero_usual) and hasText(at("07:07:00").clock()))
        waitFor(hasText(at("07:07:00").clock()) and hasText("PILO") and hasText(text(R.string.departures_usual)))
        compose.onNode(hero(R.string.departures_hero_next)).assertDoesNotExist()
    }

    @Test
    fun noTrainToCatchLeavesTheCardOut() {
        val metroOnly = morning.copy(legs = morning.legs.drop(1))
        show(FakeBackend(departures = metroOnly))
        waitFor(leg("Gare Centrale", "Mairie"))
        compose.onNode(hero(R.string.departures_hero_next)).assertDoesNotExist()
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
