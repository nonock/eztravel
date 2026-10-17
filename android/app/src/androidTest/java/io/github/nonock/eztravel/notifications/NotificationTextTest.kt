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
}
