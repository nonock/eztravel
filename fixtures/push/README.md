# Push payloads

The data part of the push messages the backend sends and the app reads, one file per notification type. Both sides test against these files, so a change here must be made on both.

All values are strings, as Firebase Cloud Messaging requires. `title` and `text` are the operator's own words, in French, as published.
