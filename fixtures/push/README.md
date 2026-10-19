# Push payloads

The data part of the push messages the backend sends and the app reads, one file per notification type. Both sides test against these files, so a change here must be made on both.

All values are strings, as Firebase Cloud Messaging requires. Line notifications (`started`, `changed`, `ended`) carry `until`, when the watch window ends, in UTC and RFC 3339: the phone removes them then, so that nothing stale stays on screen once the window is over. `title` and `text` are the operator's own words, in French, as published.
