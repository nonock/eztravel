package io.github.nonock.eztravel.backend

import org.json.JSONException
import java.io.IOException
import java.time.LocalDate
import java.util.concurrent.CopyOnWriteArrayList

/** A backend kept in memory, to test the app without a network. */
class FakeBackend(
    override val configured: Boolean = true,
    var away: LocalDate? = null,
    var failing: Boolean = false,
    /** Answers that cannot be read, as from a server in a bad state. */
    var garbled: Boolean = false,
) : Backend {
    val registered = CopyOnWriteArrayList<String>()
    val saved = CopyOnWriteArrayList<String>()

    override fun register(token: String) {
        if (failing) throw IOException("unreachable")
        registered += token
    }

    override suspend fun awayThrough(): LocalDate? {
        if (failing) throw IOException("unreachable")
        if (garbled) throw JSONException("not JSON")
        return away
    }

    override suspend fun setAwayThrough(date: LocalDate?) {
        if (failing) throw IOException("unreachable")
        saved += date.toString()
        away = date
    }
}
