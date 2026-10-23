package io.github.nonock.eztravel.backend

import io.github.nonock.eztravel.backend.CalendarDay.Kind
import io.github.nonock.eztravel.backend.CalendarDay.Why
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test
import java.io.File
import java.time.LocalDate

class CalendarDayTest {
    /** The answer the backend gives, shared with its own tests. */
    private val fixture = File("../../fixtures/api/calendar.json").readText()

    private fun day(day: Int) = LocalDate.of(2026, 11, day)

    @Test
    fun readsEachDayOfTheMonthWithWhatItIsAndWhy() {
        val days = CalendarDay.parse(fixture)
        assertEquals((1..30).map(::day), days.map { it.date })
        assertEquals(CalendarDay(day(1), Kind.AWAY, Why.AWAY), days[0])
        assertEquals(CalendarDay(day(3), Kind.OFFICE, Why.CYCLE), days[2])
        assertEquals(CalendarDay(day(4), Kind.REMOTE, Why.CYCLE), days[3])
        assertEquals(CalendarDay(day(5), Kind.REMOTE, Why.SET), days[4])
        assertEquals(CalendarDay(day(7), Kind.OFF, Why.CYCLE), days[6])
        assertEquals(CalendarDay(day(11), Kind.OFF, Why.HOLIDAY), days[10])
        assertEquals(CalendarDay(day(20), Kind.AWAY, Why.SET), days[19])
    }

    @Test
    fun valuesFromANewerBackendFallBackSafely() {
        val json = JSONObject(fixture)
        // A kind it does not know is not watched; a reason it does not know is the plan.
        json.getJSONArray("days").getJSONObject(2).put("kind", "elsewhere").put("why", "strike").put("school", true)
        assertEquals(CalendarDay(day(3), Kind.OFF, Why.CYCLE), CalendarDay.parse(json.toString())[2])
    }
}
