package io.github.nonock.eztravel.settings

import java.time.Instant
import java.time.LocalDate
import java.time.ZoneOffset

/** The Material date picker's milliseconds: the day's midnight in UTC. */
fun LocalDate.toUtcMillis(): Long = atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli()

fun Long.toUtcDate(): LocalDate = Instant.ofEpochMilli(this).atZone(ZoneOffset.UTC).toLocalDate()
