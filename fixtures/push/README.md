# Push payloads

The data part of the push messages the backend sends and the app reads, one file per notification type. Both sides test against these files, so a change here must be made on both.

All values are strings, as Firebase Cloud Messaging requires.

Train notifications (`train_cancelled`, `train_back`) name a train by its line, its `mission` code when known, its `destination`, the stop it leaves `from` and its timetable departure, `aimed`. Their `until` is that departure: the phone removes them then. `reason` says why a train is taken as cancelled: `missing` (it left its station's list before its time and stayed out for four checks in a row) or `announced` (the operator says so). `train_back` follows when a train notified as cancelled runs after all. Line notifications (`started`, `changed`, `ended`) carry `until`, when the watch window ends, in UTC and RFC 3339: the phone removes them then, so that nothing stale stays on screen once the window is over. `title` and `text` are the operator's own words, in French, as published.
