package io.github.nonock.eztravel.backend

import org.json.JSONArray
import org.json.JSONObject
import java.time.Instant

// Android's org.json may read a null as the text "null": check for it first.
internal fun JSONObject.instantOrNull(name: String): Instant? = if (isNull(name)) null else Instant.parse(getString(name))

internal fun JSONObject.stringOrNull(name: String): String? = if (isNull(name)) null else getString(name)

internal fun JSONArray.objects(): List<JSONObject> = List(length()) { getJSONObject(it) }

/** "#RRGGBB" as an opaque ARGB color; null when written otherwise. */
internal fun hexColor(text: String): Int? = text
    .takeIf { it.length == 7 && it.startsWith("#") }
    ?.substring(1)
    ?.toLongOrNull(16)
    ?.let { (0xFF000000 or it).toInt() }
