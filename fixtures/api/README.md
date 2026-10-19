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
- `legs`, in commute order: the line as in `/status`, `from` and `to` (stop names, `null` until the backend knows the line's routes) and `departures`, `null` when they cannot be read.
- Each departure: `aimed` (the timetable's time, `null` on the metro, which only gives expected times), `expected`, `arrival` at the `to` stop when known, `mission` (a train's code), `destination`, `platform`, and `status`: `on_time`, `delayed` or `cancelled`, as the operator says. A train can be said `delayed` before its expected time moves.

A train station lists both ways: a train is shown only if it also calls at the `to` stop, later. A metro platform lists one way and its next four trains.
