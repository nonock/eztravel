package io.github.nonock.eztravel.home

import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/** The time alone when it is today, with its date otherwise. */
fun Instant.shown(
    today: LocalDate = LocalDate.now(),
    zone: ZoneId = ZoneId.systemDefault(),
    locale: Locale = Locale.getDefault(),
): String {
    val local = atZone(zone)
    val format = if (local.toLocalDate() == today) {
        DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)
    } else {
        DateTimeFormatter.ofLocalizedDateTime(FormatStyle.FULL, FormatStyle.SHORT)
    }
    return format.withLocale(locale).format(local)
}

/** The time alone, whatever the day: departures are a few hours ahead at most. */
fun Instant.clock(zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String =
    DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale).format(atZone(zone))

/** Whole minutes from [now] to this time, rounded down; 0 once it is past. */
fun Instant.minutesFrom(now: Instant): Long = Duration.between(now, this).toMinutes().coerceAtLeast(0)
