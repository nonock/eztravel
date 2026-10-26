# eztravel

[![CI](https://github.com/nonock/eztravel/actions/workflows/ci.yml/badge.svg)](https://github.com/nonock/eztravel/actions/workflows/ci.yml)

An Android app that follows one home-to-work commute in the Paris region and notifies you when its lines are disrupted, only on the days and at the hours you travel. Data comes from [PRIM](https://prim.iledefrance-mobilites.fr), the open data portal of Île-de-France Mobilités.

> **Status: milestone 3 done.** Disruption alerts on office days, next departures, cancelled trains and the reminder to leave; a month calendar with school holidays, and the phone checking the backend. Next: backup routes (milestone 4).

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
- a few minutes before it is time to leave: for the train picked in the app that day, or else the way's usual train, set in the app (the first train from its time that can be caught, leaving at most half an hour after it was time to), with its platform and how it runs as far as PRIM knows; then again if that train runs three minutes late or more. A picked train that is cancelled gives way to the next one;
- when no check has succeeded for ten minutes ("watching interrupted"), then when checks work again.

A line's notification goes away by itself when its window ends. Every notification sent is kept in a history, with the PRIM records behind it, and every train followed in a journal, with how it went (on time, late, cancelled), for the statistics to come.

**The app** shows the notifications, titled in English or French. A line's says in one line where the disruption is (between two stops, at a stop, or the whole line), and a tap opens its record, whole, on the Alerts tab. It has two tabs. Beside the gear, an icon says how the watch goes: nothing watched now, all fine (only when the way is watched, just checked, and every line runs), a line in trouble, or that checks fail; a tap opens all of it, the lines being watched, each with its color and state, the last check, the next watch window and what is planned today. **Commute** keeps a banner only when something is wrong, and, during school holidays, a line saying fewer trains run. A card gives the train to catch, picked or next, with a large countdown to when to leave, and below come the next departures on each leg of the way, a train a row: when it leaves and arrives, its destination, then its platform, delay or cancellation; a metro's next trains fit on one line. With leave to know roughly where the phone is, asked only while the app shows and kept on the phone, the leg boarding nearest comes first. On the ride with a time to the platform, the trains leaving too soon to be caught from home or work still show when they leave, for whoever is already on the way, then three that can still be caught, and the rest on request. The backend reads them from PRIM when the app asks, at most once a minute per stop. A train of the ride with a time to the platform can be picked with its bell for the reminder to leave, if it leaves during the window of an office day; without a pick, the way's usual train is the one shown first. While a line of the way is disrupted on the leg itself, for sure, by an incident under way or by planned works that cut it, its backups follow, when configured: rides on other lines, with their next departures, each said disrupted too when it is; a backup with a ride cut as well is left out. **Alerts** lists every disruption of the lines not over yet, under way first, with its period and whole text, and whether it touches the commute. The gear opens three pages, and, until the app may set exact alarms, a row that opens Android's settings to allow "Alarms & reminders": without it, the phone's own looks at the backend (below) may come up to an hour late. **Calendar** shows a month at a time, a week a row: office days, remote-work days and days off each in their own color, public holidays outlined, school holidays marked; a tap on a day, from today up to a year ahead, sets it to office, remote work or day off, or back as planned, or sets days off from it through a date chosen. **Usual trains** holds the usual train of each way, by its time. **History** keeps the notifications sent. During the watch windows the phone also looks at the backend by itself, every twenty minutes or so, and warns when it does not answer, has stopped checking or cannot send notifications, which the backend cannot say itself. A splash screen shows the logo.

## Architecture

| Part | Role | Technology |
|---|---|---|
| Backend | Polls PRIM during the watch windows, applies the office-day calendar, places disruptions on the commute's branches, decides what to notify, keeps state and history | Rust (tokio, axum, reqwest, jiff), JSON files on a volume, fly.io |
| Push | Delivers notifications to the phone | Firebase Cloud Messaging (HTTP v1) |
| App | Notifications, home screen, calendar, history | Kotlin, Jetpack Compose |

The logic lives in the backend; the app only displays. Two reasons: Android cannot reliably watch a line in the background, and the PRIM key cannot ship inside an app whose code is public. The backend is not an open relay: the app authenticates to it with a key compiled into the user's own build.

```
backend/    Rust: PRIM polling, rules, push, API
android/    Kotlin: notifications, screens
fixtures/   push payloads and API answers, shared by the tests of both sides
```

## Running your own

You need a free [PRIM](https://prim.iledefrance-mobilites.fr) account and API key, a [Firebase](https://console.firebase.google.com) project, a [fly.io](https://fly.io) account (the backend fits in the smallest machine, about $2 a month) and Android Studio.

### 1. Describe your commute

Copy [backend/config.example.toml](backend/config.example.toml) outside the repository and describe your own commute: the office-day calendar (two alternating weeks), the watch windows, the lines and the legs of each way. Lines and stops are Navitia ids (`line:IDFM:C01740` is written `C01740`; stops are `stop_area:IDFM:…`), which PRIM's Navitia API gives. Each line may have a color (`"#RRGGBB"`, Navitia's `color` field) for the home screen. A window may start following trains earlier (`trains_from`): PRIM often drops a cancelled train half an hour or more before its time, and such trains are then notified when the window opens. The school holidays of a zone (`[school_holidays]`, as [data.education.gouv.fr](https://data.education.gouv.fr/explore/dataset/fr-en-calendrier-scolaire/) names its zones and places) are optional: the backend reads them once a day, and the app shows them. So are backups (`[[backups]]`): rides on other lines that stand in for some lines of a way, shown with their next departures while one of those lines is disrupted on that way.

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

Then build and install it from Android Studio, or with `./gradlew installDebug`. At first launch it asks for permission to notify and registers with the backend. For the phone's own looks at the backend to come on time, allow "Alarms & reminders" from the row behind the gear.

## API

All requests carry `Authorization: Bearer <APP_KEY>`.

| Request | |
|---|---|
| `POST /devices` `{"token": "…"}` | Registers the phone's Firebase token |
| `GET /settings` | The usual trains: [fixtures/api/README.md](fixtures/api/README.md) |
| `PUT /usual-train` `{"direction": "outbound", "time": "08:07"}` | Sets the usual train of a way, or `null` for none: [fixtures/api/README.md](fixtures/api/README.md) |
| `GET /calendar?month=2026-11` | Each day of the month, what it is and why: [fixtures/api/README.md](fixtures/api/README.md) |
| `PUT /calendar` `{"date": "2026-11-05", "kind": "remote"}` | Sets a day, or every day `through` another, or `null` to put them back as planned: [fixtures/api/README.md](fixtures/api/README.md) |
| `GET /status` | What the backend sees: [fixtures/api/README.md](fixtures/api/README.md) |
| `GET /history` | What it sent, newest first: [fixtures/api/history.json](fixtures/api/history.json) |
| `GET /departures` | The next departures on each leg: [fixtures/api/README.md](fixtures/api/README.md) |
| `GET /alerts` | Every disruption of the lines not yet over: [fixtures/api/README.md](fixtures/api/README.md) |
| `GET /trains` | Each train followed, as it ended, the latest first: [fixtures/api/README.md](fixtures/api/README.md) |
| `PUT /departure` `{"journey": "…"}` | Picks a departure for the reminder to leave, or `null` for none: [fixtures/api/README.md](fixtures/api/README.md) |

Push payloads are described in [fixtures/push/README.md](fixtures/push/README.md).

## Development

- Backend, from `backend/`: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt`, coverage with `cargo llvm-cov`.
- App, from `android/`: `./gradlew testDebugUnitTest` (JVM), `./gradlew connectedDebugAndroidTest` (on a phone, unlocked), `./gradlew lintDebug`, coverage with `createDebugUnitTestCoverageReport` and `createDebugAndroidTestCoverageReport`.
- On GitHub, [CI](.github/workflows/ci.yml) runs the same checks at each push and pull request, the tests on a device included, on an emulator that Gradle starts (`./gradlew ciDeviceDebugAndroidTest`); `main` only changes through a pull request that passes them all, and [Dependabot](.github/dependabot.yml) proposes updates once a month, of releases at least two weeks old.
- Git hooks, enabled with `git config core.hooksPath hooks`: before a commit, [hooks/check.py](hooks/check.py) refuses secrets (the values of `.env`, `local.properties` and the Firebase files, and keys of known formats) and personal data (the words of `local/forbidden.txt` and the stops and lines of your configuration), and Rust code left unformatted; before a push, it checks every commit pushed, authors included, and that each dependency version added is two weeks old, or fixes a vulnerability of the one it replaces ([hooks/ages.py](hooks/ages.py), exceptions in [hooks/fresh-allowed.txt](hooks/fresh-allowed.txt)), then runs the backend's lint, tests and `cargo audit`, and the app's JVM tests and lint. They need Python 3, `cargo install cargo-audit` and `JAVA_HOME`. Their own tests: `python -m unittest discover -s hooks`.

## Roadmap

0. **Done.** PRIM data checked against the lines' X accounts.
1. **Done.** Disruption alerts on office days: backend, calendar, push, app.
2. **Done.** Next departures, cancelled trains, "leave in X min" countdown, notification before departure, and the usual train of each way, set once in the app.
3. **Done.** Trust and calendar:
   - when a window opens, the phone checks the backend itself and says so if it does not answer or has stopped checking, since the backend cannot report its own failure;
   - a month calendar in the app, with office days, remote-work days and days off each in their own color, where a tap sets a single day: office, remote work or day off, or days off through a date;
   - school holidays on that calendar, from the French open data: some lines run fewer trains then, and the app says so on those days (PRIM's timetables for the Toussaint 2026 show 15 % fewer J trains and 11 % fewer metro 12 trains in the example windows);
   - each day's trains recorded (delay, cancelled) for the statistics, kept beyond the history;
   - continuous integration on GitHub: backend and app tests, lint.
4. Preconfigured backup routes: when a line of the way is disrupted, the next departures of its backup on the commute tab.
5. Shorter notifications: one line built from the data (where, until when), and the whole record in the app on tap. The links in operators' texts (the line's X account, its blog) open from the app. Plus an accessibility check of the screens.
6. Sharper alerts:
   - trains that never show up in real time, found against the timetable, which also stands in when real time is missing;
   - disruptions placed on the branches more finely;
   - the evening before, the planned works or strikes of the next day, with a "remote work tomorrow" button.
7. Live trip, from the reminder to leave:
   - an ongoing notification (a Live Update where Android allows it) and a widget, with the train to catch, its countdown, platform and delay;
   - updates pushed by the backend only when something changes;
   - at a terminus PRIM gives the platform only minutes before departure, so it is shown as soon as it is known;
   - a train can be picked further ahead than the next five departures.
8. Statistics, from the days recorded since milestone 3.

Later: location in the background, to start the live trip on leaving home or work or on reaching the station (it may add a notification, never silence one); free route planning, several saved commutes.

## Project rules

- **TDD.** The test is written before the code, and coverage is complete, UI included.
- **English code, localized app.** Code, comments, tests, commit messages and docs are in English. User-facing text lives in string resources: English is the base language and French is the first translation. Messages published by the transport operators are shown as they are, in French.
- **No secrets and no personal data in the repository.** Keys go in `.env` (see [.env.example](.env.example)), `local.properties` and fly.io secrets. Each user's commute, schedule and calendar are configuration, never code; tests use a made-up commute.

## Data

Île-de-France Mobilités data comes with its own terms (the "Licence Mobilités" for most APIs). A free PRIM account and an API key are required.

## License

Code is under the [MIT](LICENSE) license. The app's icons come from Google's [Material icons](https://fonts.google.com/icons), under the Apache License 2.0.
