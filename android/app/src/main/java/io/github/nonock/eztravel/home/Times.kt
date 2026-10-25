package io.github.nonock.eztravel.home

import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.chrono.IsoChronology
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeFormatterBuilder
import java.time.format.FormatStyle
import java.util.Locale

/** The time alone when it is today, with its short date otherwise: "jeu. 5 nov. 06:45". */
fun Instant.shown(
    today: LocalDate = LocalDate.now(),
    zone: ZoneId = ZoneId.systemDefault(),
    locale: Locale = Locale.getDefault(),
): String {
    val local = atZone(zone)
    val time = local.clock(locale)
    if (local.toLocalDate() == today) return time
    return DateTimeFormatter.ofPattern("EEE ${dayAndMonth(locale)}", locale).format(local) + " " + time
}

/** The day and month: "17 oct.", as the locale writes them. */
fun LocalDate.shortDate(locale: Locale = Locale.getDefault()): String =
    DateTimeFormatter.ofPattern(dayAndMonth(locale), locale).format(this)

/** The locale's medium date, without the year: the coming weeks are meant. */
private fun dayAndMonth(locale: Locale): String =
    DateTimeFormatterBuilder.getLocalizedDateTimePattern(FormatStyle.MEDIUM, null, IsoChronology.INSTANCE, locale)
        .replace(Regex("[\\s,]*y+[\\s.,]*"), "")
        .trim()

private fun ZonedDateTime.clock(locale: Locale): String =
    DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale).format(this)

/** The time alone, whatever the day: departures are a few hours ahead at most. */
fun Instant.clock(zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String =
    atZone(zone).clock(locale)

/** Whole minutes from [now] to this time, rounded down; 0 once it is past. */
fun Instant.minutesFrom(now: Instant): Long = Duration.between(now, this).toMinutes().coerceAtLeast(0)
