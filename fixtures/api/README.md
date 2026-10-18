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
