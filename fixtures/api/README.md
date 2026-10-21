# API answers

What the backend answers and the app reads, one file per case. Both sides test against these files, so a change here must be made on both.

`GET /status` tells the home screen what the backend sees:

- `watching`: `outbound`, `inbound`, or `null` outside the watch windows.
- `checked_at`: the last complete check, when PRIM was read and every notification sent.
- `next`: when the next watch window opens.
- `outage`: `null`, or since when no check has been complete and why: `prim` (PRIM cannot be read), `firebase` (notifications cannot be sent) or `server` (the backend stopped checking).
- `lines`: the lines of the way being watched, with their `color` (`#RRGGBB`, or `null` when not configured) and `severity` `blocking`, `disrupted`, or `null` when fine.
- `planned`: the titles of the planned disruptions announced today.

Times are UTC, in RFC 3339.

`GET /departures` gives the next departures on each leg of the way shown: the way to work until its watch window ends, the way home after (`departures.json`, `departures-unavailable.json`):

- `direction`: `outbound` or `inbound`.
- `legs`, in commute order: the line as in `/status`, `from` and `to` (stop names, `null` until the backend knows the line's routes), `reach` (minutes from leaving home or work to the platform, `null` when not configured: the app counts down to when to leave) and `departures`, `null` when they cannot be read.
- Each departure: `journey` (the train or metro, the same at every stop of its run), `aimed` (the timetable's time, `null` on the metro, which only gives expected times), `expected`, `arrival` at the `to` stop when known, `mission` (a train's code), `destination`, `platform`, and `status`: `on_time`, `delayed` or `cancelled`, as the operator says, `picked`, true for the train picked for the reminder to leave, and `usual`, true on the first ride with a `reach` for the way's usual train when none is picked that day: the first not cancelled whose `expected` time is not before the usual one. A train can be said `delayed` before its expected time moves.

A train station lists both ways: a train is shown only if it also calls at the `to` stop, later, or ends there (a terminus drops trains from its arrivals during incidents; `arrival` is then `null`). A metro platform lists one way and its next four trains. A leg shows its next five departures; with a `reach`, those leaving too soon to be caught from home or work come first, for whoever is already on the way, and the five can still be caught.

`PUT /departure` `{"journey": "…"}` picks a departure of the way shown for the reminder to leave, instead of the usual train, for the day; `{"journey": null}` drops the pick. Only a train of the first ride with a `reach` can be picked: the time to leave is its expected time less the `reach`. The answer is `204`, or `422` with the reason (`pick-refused.json`): `too_late` (that time is past), `not_office_day` (or time away), `outside_window` (no reminder is sent then), `unknown` (not a train shown on that ride, or cancelled) or `unavailable` (the departures cannot be read just now).

`GET /settings` gives the last day away (`away_through`, a date or `null`) and `usual_trains`: for each way, `outbound` and `inbound`, the time of its usual train at the station of the first ride with a `reach`, local, or `null` (`settings.json`). On office days the reminder to leave is for the first train from that time that can be caught, unless one is picked that day. `PUT /usual-train` `{"direction": "outbound", "time": "08:07"}` sets one way's usual train, `"time": null` clears it: `204`, or `422` with the reason as for a pick, `outside_window` when leaving for it would fall outside the way's window, `unknown` when the way has no ride with a `reach`.

`GET /trains` gives each train followed, as it ended, the latest first, a thousand at most (`trains.json`): kept for the statistics, beyond the history. A train ends half an hour after its time, or when the next window is watched or observed. Each has its `day` and `direction` (the window's), `line`, `mission`, `destination`, `from` (the stop it leaves from), `aimed` and `expected` (its departure in the timetable, and as last listed), `outcome`: `ran` (listed until it could have left), `missing` (out of the list four polls in a row before its time) or `announced` (said cancelled, last it was listed), and `came_back`: notified as cancelled, then listed as running again.

`GET /alerts` gives every disruption of the configured lines that is not over yet, for the app's alerts page (`alerts.json`). `lines` is `null` when PRIM's feed cannot be read; otherwise each line, in the configuration's order, as in `/status`, with its `alerts`: those under way first (`active`), the worst first (`severity`: `blocking`, `disrupted` or `information`), then those to come by their start. Each alert has its `id`, `cause` (`works`, `incident`, `information` or `unknown`), `title`, the operator's whole `text` as plain text, `begin` and `end` of the period under way or else of the next one, `updated`, and `on_way`: true when it touches a leg of the commute, so that it is notified during a window. The feed is shared with the watch: the backend reuses the watch's last read, or reads again at most every five minutes.
