package io.github.nonock.eztravel.notifications

import android.content.res.Configuration
import android.content.res.Resources
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.nonock.eztravel.notifications.PushMessage.Line
import io.github.nonock.eztravel.notifications.PushMessage.Mode
import io.github.nonock.eztravel.notifications.PushMessage.Severity
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

@RunWith(AndroidJUnit4::class)
class NotificationTextTest {
    private fun resources(language: String): Resources {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val configuration = Configuration(context.resources.configuration)
        configuration.setLocale(Locale.forLanguageTag(language))
        return context.createConfigurationContext(configuration).resources
    }

    private val english = resources("en")
    private val french = resources("fr")
    private val train = Line("L", Mode.TRAIN)
    private val metro = Line("X", Mode.METRO)
    private val operatorTitle = "Ligne L : Nanterre <-> Cergy trafic interrompu"
    private val operatorText = "Le trafic est interrompu entre Nanterre et Cergy.\nMotif : panne d'un train."

    @Test
    fun aStartIsTitledInThePhoneLanguageWithTheOperatorTextAsBody() {
        val started = PushMessage.Started(train, Severity.BLOCKING, operatorTitle, operatorText)
        assertEquals(NotificationText("Line L: service interrupted", operatorText), started.text(english))
        assertEquals(NotificationText("Ligne L : trafic interrompu", operatorText), started.text(french))
    }

    @Test
    fun aDisruptedMetro() {
        val started = PushMessage.Started(metro, Severity.DISRUPTED, operatorTitle, operatorText)
        assertEquals("Metro X: service disrupted", started.text(english).title)
        assertEquals("Métro X : trafic perturbé", started.text(french).title)
    }

    @Test
    fun withoutTextTheOperatorTitleIsTheBody() {
        val titleOnly = PushMessage.Started(train, Severity.DISRUPTED, operatorTitle, null)
        assertEquals(operatorTitle, titleOnly.text(french).body)
        val nothing = PushMessage.Started(train, Severity.DISRUPTED, null, null)
        assertEquals("", nothing.text(french).body)
    }

    @Test
    fun changesSayWhichWay() {
        val worse = PushMessage.Changed(train, Severity.BLOCKING, Severity.DISRUPTED, operatorTitle, operatorText)
        assertEquals(NotificationText("Line L: now interrupted", operatorText), worse.text(english))
        assertEquals("Ligne L : trafic désormais interrompu", worse.text(french).title)
        val better = PushMessage.Changed(metro, Severity.DISRUPTED, Severity.BLOCKING, null, "Reprise progressive.")
        assertEquals(NotificationText("Metro X: running again, still disrupted", "Reprise progressive."), better.text(english))
        assertEquals("Métro X : reprise, trafic encore perturbé", better.text(french).title)
        val untitled = PushMessage.Changed(metro, Severity.DISRUPTED, Severity.BLOCKING, operatorTitle, null)
        assertEquals(operatorTitle, untitled.text(french).body)
    }

    private val paris = ZoneId.of("Europe/Paris")
    private val palo = PushMessage.Train(train, "PALO", "Gare Centrale", "Les Aubépines", Instant.parse("2026-11-03T06:52:00Z"))

    @Test
    fun aCancelledTrainIsNamedByItsTimeAndSaysHowItIsKnown() {
        val missing = PushMessage.TrainCancelled(palo, PushMessage.Reason.MISSING, null)
        assertEquals(
            NotificationText("Ligne L : train de 07:52 supprimé", "PALO vers Gare Centrale, depuis Les Aubépines : il n’est plus annoncé."),
            missing.text(french, paris),
        )
        val sevenFiftyTwo = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(Locale.ENGLISH).format(palo.aimed.atZone(paris))
        assertEquals(
            NotificationText("Line L: $sevenFiftyTwo train cancelled", "PALO to Gare Centrale, from Les Aubépines: no longer announced."),
            missing.text(english, paris),
        )
        val announced = PushMessage.TrainCancelled(palo, PushMessage.Reason.ANNOUNCED, null)
        assertEquals("PALO vers Gare Centrale, depuis Les Aubépines : annoncé supprimé.", announced.text(french, paris).body)
        assertEquals("PALO to Gare Centrale, from Les Aubépines: announced as cancelled.", announced.text(english, paris).body)
    }

    @Test
    fun aTrainBackSaysItRunsAfterAll() {
        val back = PushMessage.TrainBack(palo.copy(mission = null), null)
        assertEquals(NotificationText("Ligne L : train de 07:52 finalement maintenu", "Train vers Gare Centrale, depuis Les Aubépines."), back.text(french, paris))
        assertEquals("Train to Gare Centrale, from Les Aubépines.", back.text(english, paris).body)
    }

    @Test
    fun anEndHasATitleOnly() {
        val ended = PushMessage.Ended(train)
        assertEquals(NotificationText("Line L: back to normal", ""), ended.text(english))
        assertEquals(NotificationText("Ligne L : trafic rétabli", ""), ended.text(french))
        assertEquals("Ligne C09999 : trafic rétabli", PushMessage.Ended(Line("C09999", Mode.OTHER)).text(french).title)
    }

    @Test
    fun plannedDisruptionsAreCountedAndListed() {
        val one = PushMessage.Planned(1, "Ligne L : offre adaptée")
        assertEquals(NotificationText("1 planned disruption on your lines today", "Ligne L : offre adaptée"), one.text(english))
        assertEquals("1 perturbation prévue sur vos lignes aujourd’hui", one.text(french).title)
        val two = PushMessage.Planned(2, "Ligne L : offre adaptée\nMétro X : station fermée")
        assertEquals("2 planned disruptions on your lines today", two.text(english).title)
        assertEquals("2 perturbations prévues sur vos lignes aujourd’hui", two.text(french).title)
    }

    @Test
    fun anOutageSaysThatSilenceMeansNothing() {
        assertEquals(
            NotificationText(
                "Watching interrupted",
                "No successful check for 10 minutes: no notification no longer means no disruption. Check the traffic elsewhere.",
            ),
            PushMessage.Outage.text(english),
        )
        assertEquals(
            NotificationText(
                "Surveillance interrompue",
                "Aucun relevé réussi depuis 10 minutes : sans notification, rien ne garantit que le trafic est normal. Vérifiez-le ailleurs.",
            ),
            PushMessage.Outage.text(french),
        )
    }

    @Test
    fun theEndOfAnOutageSaysWatchingIsBack() {
        assertEquals(NotificationText("Watching restored", "Checks are working again."), PushMessage.Recovered.text(english))
        assertEquals(NotificationText("Surveillance rétablie", "Les relevés fonctionnent de nouveau."), PushMessage.Recovered.text(french))
    }
}
