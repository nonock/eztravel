package io.github.nonock.eztravel.home

import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.nonock.eztravel.R
import io.github.nonock.eztravel.backend.Departures
import io.github.nonock.eztravel.backend.Departures.Departure
import io.github.nonock.eztravel.backend.Departures.Leg
import io.github.nonock.eztravel.backend.Departures.State
import io.github.nonock.eztravel.backend.FakeBackend
import io.github.nonock.eztravel.backend.Status.Direction
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
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
                    Departure(at("06:52:00"), at("06:52:00"), at("07:04:00"), "PALO", "Gare Centrale", "2", State.ON_TIME),
                    Departure(at("07:07:00"), at("07:11:00"), at("07:23:00"), "PILO", "Gare Centrale", "2", State.DELAYED),
                    Departure(at("07:22:00"), at("07:22:00"), at("07:34:00"), "PALO", "Gare Centrale", "2", State.CANCELLED),
                    // Said late, with no time yet.
                    Departure(at("07:37:00"), at("07:37:00"), null, null, "Les Tilleuls", null, State.DELAYED),
                ),
            ),
            Leg(
                metro,
                0xFFE07A1F.toInt(),
                "Gare Centrale",
                "Mairie",
                listOf(
                    Departure(null, at("06:50:20"), null, null, "Porte Nord", null, State.ON_TIME),
                    Departure(null, at("06:53:30"), null, null, "Porte Nord", null, State.ON_TIME),
                ),
            ),
        ),
    )

    @Test
    fun eachLegShowsItsNextDepartures() {
        show(FakeBackend(departures = morning))
        waitFor(text(R.string.departures_title))
        waitFor(hasContentDescription(text(R.string.line_train, "L")) and hasText(text(R.string.departures_leg, "Les Aubépines", "Gare Centrale")))
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
        waitFor(hasContentDescription(text(R.string.line_metro, "X")) and hasText(text(R.string.departures_leg, "Gare Centrale", "Mairie")))
        waitFor(hasText(text(R.string.departures_arriving)) and hasText("Porte Nord"))
        waitFor(hasText(text(R.string.departures_minutes, 3)) and hasText("Porte Nord"))
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
        waitFor(hasText(text(R.string.departures_leg, "Mairie", "Gare Centrale")))
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
