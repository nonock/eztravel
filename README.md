# eztravel

An Android app that follows one home-to-work commute in the Paris region: disruption alerts for the lines you take, real-time next departures, and a suggested time to leave. Data comes from [PRIM](https://prim.iledefrance-mobilites.fr), the open data portal of Île-de-France Mobilités.

> **Status: milestone 1 in progress.** Milestone 0 compared PRIM with the lines' X accounts for a week: PRIM carries the same incident texts within a few minutes, and shows cancelled trains as early or earlier. PRIM is the only data source.

## Why

Existing apps cover the whole network, for everyone. eztravel does the opposite: one commute, its user's.

- Only the lines of the commute are watched.
- Alerts arrive as notifications, even when the app is closed.
- Nothing is watched or notified on remote-work days, public holidays or time off.
- When a line is disrupted, the app suggests the backup route planned in advance.

## Architecture

| Part | Role | Technology |
|---|---|---|
| Backend | Polls PRIM during commute windows, applies the office-day calendar, filters disruptions, avoids duplicate alerts, picks the backup route, computes the time to leave, keeps history | Rust (axum), SQLite, fly.io |
| Push | Delivers alerts to the phone | Firebase Cloud Messaging |
| App | A home screen readable at a glance, disruption details, settings, statistics | Kotlin, Jetpack Compose |
| Widget | Next departure and line status on the home screen | Jetpack Glance |

The logic lives in the backend; the app only displays. Two reasons: Android cannot reliably watch a line in the background, and the PRIM key cannot ship inside an app whose code is public.

```
backend/    Rust: API, PRIM polling, rules, push
android/    Kotlin: screens, widget, notifications
fixtures/   recorded PRIM responses, replayed in tests
```

## Roadmap

| Milestone | Content |
|---|---|
| 0 | PRIM data check, no code |
| 1 | Disruption alert on office days: backend, calendar, push, minimal app |
| 2 | Next departures, "leave in X min" countdown, notification before departure |
| 3 | Widget |
| 4 | Preconfigured backup routes |
| 5 | History and statistics |

Later: free route planning, several saved commutes.

## Project rules

- **TDD.** The test is written before the code, and coverage is complete, UI included.
- **English code, localized app.** Code, comments, tests, commit messages and docs are in English. User-facing text lives in string resources: English is the base language and French is the first translation. Messages published by the transport operators are shown as they are, in French.
- **No secrets and no personal data in the repository.** The PRIM key goes in `.env` (see [.env.example](.env.example)). Each user's commute, schedule and calendar are configuration, never code.
- **The backend is not an open relay.** The app authenticates to it, so that nobody else can use up the PRIM quota.

## Data

Île-de-France Mobilités data comes with its own terms (the "Licence Mobilités" for most APIs). A free PRIM account and an API key are required.

## License

Code is under the [MIT](LICENSE) license.
