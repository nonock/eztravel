# eztravel

An Android app that follows one home-to-work commute in the Paris region and notifies you when its lines are disrupted, only on the days and at the hours you travel. Data comes from [PRIM](https://prim.iledefrance-mobilites.fr), the open data portal of Île-de-France Mobilités.

> **Status: milestone 1 done.** Disruption alerts on office days. Next: real-time departures and cancelled trains (milestone 2).

## Why

Existing apps cover the whole network, for everyone. eztravel does the opposite: one commute, its user's.

- Only the lines of the commute are watched, and only the branches of each line that the commute uses.
- Alerts arrive as notifications, even when the app is closed.
- Nothing is watched or notified on remote-work days, public holidays or time off.
- When no notification comes, it means nothing is wrong: if the backend cannot check, it says so.

## What it does

**The backend** reads PRIM's disruptions every two minutes during the watch windows (for example 6:45–8:15 and 16:00–17:30), on office days only. It keeps the disruptions that touch a leg of the commute and notifies:

- when a line becomes disrupted, or is already disrupted when a window opens;
- when its severity changes (disrupted ↔ interrupted);
- when it is back to normal, if its start was notified;
- once a day, the planned disruptions (works, adapted service) that affect the commute;
- when a train due to leave during the window is cancelled: it left its station's list before its time and stayed out for four checks in a row, or the operator says so; then again if it runs after all. Only train lines are followed this way: a metro platform lists its next four trains, without their timetable;
- a few minutes before it is time to leave: for the train picked in the app that day, or else, when the way has a usual time to set off, for the first train that can be caught (leaving at most half an hour after that time), with its platform and how it runs as far as PRIM knows; then again if that train runs three minutes late or more. A picked train that is cancelled gives way to the next one;
- when no check has succeeded for ten minutes ("watching interrupted"), then when checks work again.

A line's notification goes away by itself when its window ends. Every notification sent is kept in a history, with the PRIM records behind it.

**The app** shows the notifications, titled in English or French, with the operator's text as published. It has three tabs. **Commute** opens on what to know first: all fine (only when the way is watched, just checked, and every line runs), the line in trouble, or that checks fail; then the lines being watched, each with its color and state, the last check and the next watch window. A card gives the train to catch, picked or next, with a large countdown to when to leave, and below come the next departures on each leg of the way, with their delay or cancellation, platform and arrival time; the backend reads them from PRIM when the app asks, at most once a minute per stop. A train of the ride with a time to the platform can be picked with its bell for the reminder to leave, if it leaves during the window of an office day. **Alerts** lists every disruption of the lines not over yet, under way first, with its period and whole text, and whether it touches the commute. **History** keeps the notifications sent. The time away ("away until…") is in the settings, behind the gear. A splash screen shows the logo.

## Architecture

| Part | Role | Technology |
|---|---|---|
| Backend | Polls PRIM during the watch windows, applies the office-day calendar, places disruptions on the commute's branches, decides what to notify, keeps state and history | Rust (tokio, axum, reqwest, jiff), JSON files on a volume, fly.io |
| Push | Delivers notifications to the phone | Firebase Cloud Messaging (HTTP v1) |
| App | Notifications, home screen, time away, history | Kotlin, Jetpack Compose |

The logic lives in the backend; the app only displays. Two reasons: Android cannot reliably watch a line in the background, and the PRIM key cannot ship inside an app whose code is public. The backend is not an open relay: the app authenticates to it with a key compiled into the user's own build.

```
backend/    Rust: PRIM polling, rules, push, API
android/    Kotlin: notifications, screens
fixtures/   push payloads and API answers, shared by the tests of both sides
```

## Running your own

You need a free [PRIM](https://prim.iledefrance-mobilites.fr) account and API key, a [Firebase](https://console.firebase.google.com) project, a [fly.io](https://fly.io) account (the backend fits in the smallest machine, about $2 a month) and Android Studio.

### 1. Describe your commute

Copy [backend/config.example.toml](backend/config.example.toml) outside the repository and describe your own commute: the office-day calendar (two alternating weeks), the watch windows, the lines and the legs of each way. Lines and stops are Navitia ids (`line:IDFM:C01740` is written `C01740`; stops are `stop_area:IDFM:…`), which PRIM's Navitia API gives. Each line may have a color (`"#RRGGBB"`, Navitia's `color` field) for the home screen. A window may start following trains earlier (`trains_from`): PRIM often drops a cancelled train half an hour or more before its time, and such trains are then notified when the window opens.

### 2. Firebase

Create a Firebase project and add an Android app to it with the package name `io.github.nonock.eztravel` (or change `applicationId` in [android/app/build.gradle.kts](android/app/build.gradle.kts)). Put its `google-services.json` in `android/app/`. In the project settings, create a service account key for the backend.

### 3. The backend on fly.io

Change `app` in [backend/fly.toml](backend/fly.toml) to a name of your own, then from `backend/`:

```sh
fly apps create <your app>
fly volumes create eztravel_data --region cdg --size 1
fly ips allocate-v4 --shared
fly ips allocate-v6
fly secrets import --stage   # see the variables below
fly deploy --ha=false
```

Allocate the IP addresses before the first call to the app's name: `fly apps create` gives none, and some DNS resolvers remember for a while that the name had no address.

The backend reads its configuration from environment variables, set as fly.io secrets:

| Variable | Content |
|---|---|
| `EZTRAVEL_CONFIG` | The text of your configuration file |
| `PRIM_API_KEY` | Your PRIM key |
| `FIREBASE_SERVICE_ACCOUNT` | The text of the service account key file |
| `APP_KEY` | A random key of 32 characters or more, shared with the app |
| `DEVICE_TOKENS` | Optional: Firebase tokens to start with (the app registers itself) |
| `EZTRAVEL_DATA` | Where state, history and routes are kept (`/data` in `fly.toml`) |

Its log starts with the local time and what it is watching: `fly logs`.

### 4. The app

In `android/local.properties` (never committed), give the app the backend's address and key:

```properties
eztravel.backendUrl=https://<your app>.fly.dev
eztravel.appKey=<the same APP_KEY>
```

Then build and install it from Android Studio, or with `./gradlew installDebug`. At first launch it asks for permission to notify and registers with the backend.

## API

All requests carry `Authorization: Bearer <APP_KEY>`.

| Request | |
|---|---|
| `POST /devices` `{"token": "…"}` | Registers the phone's Firebase token |
| `GET /settings`, `PUT /settings` `{"away_through": "2026-12-24"}` | The last day away, or `null` |
| `GET /status` | What the backend sees: [fixtures/api/README.md](fixtures/api/README.md) |
| `GET /history` | What it sent, newest first: [fixtures/api/history.json](fixtures/api/history.json) |
| `GET /departures` | The next departures on each leg: [fixtures/api/README.md](fixtures/api/README.md) |
| `GET /alerts` | Every disruption of the lines not yet over: [fixtures/api/README.md](fixtures/api/README.md) |
| `PUT /departure` `{"journey": "…"}` | Picks a departure for the reminder to leave, or `null` for none: [fixtures/api/README.md](fixtures/api/README.md) |

Push payloads are described in [fixtures/push/README.md](fixtures/push/README.md).

## Development

- Backend, from `backend/`: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt`, coverage with `cargo llvm-cov`.
- App, from `android/`: `./gradlew testDebugUnitTest` (JVM), `./gradlew connectedDebugAndroidTest` (on a phone, unlocked), `./gradlew lintDebug`, coverage with `createDebugUnitTestCoverageReport` and `createDebugAndroidTestCoverageReport`.

## Roadmap

| Milestone | Content | |
|---|---|---|
| 0 | PRIM data check against the lines' X accounts | done |
| 1 | Disruption alerts on office days: backend, calendar, push, app | done |
| 2 | Next departures, cancelled trains, "leave in X min" countdown, notification before departure | |
| 3 | Widget | |
| 4 | Preconfigured backup routes | |
| 5 | Statistics | |

Later: free route planning, several saved commutes.

## Project rules

- **TDD.** The test is written before the code, and coverage is complete, UI included.
- **English code, localized app.** Code, comments, tests, commit messages and docs are in English. User-facing text lives in string resources: English is the base language and French is the first translation. Messages published by the transport operators are shown as they are, in French.
- **No secrets and no personal data in the repository.** Keys go in `.env` (see [.env.example](.env.example)), `local.properties` and fly.io secrets. Each user's commute, schedule and calendar are configuration, never code; tests use a made-up commute.

## Data

Île-de-France Mobilités data comes with its own terms (the "Licence Mobilités" for most APIs). A free PRIM account and an API key are required.

## License

Code is under the [MIT](LICENSE) license. The app's icons come from Google's [Material icons](https://fonts.google.com/icons), under the Apache License 2.0.
