# coach (Android)

The `coach.xinutec.org` app presented as a native app — a full-screen **WebView**,
no address bar, a home-screen icon — **plus** a native **home geofence** that nudges
you to train when you're home.

The site is behind Nextcloud-identity login; the WebView keeps the session cookie,
so it's a one-time sign-in. It is served on the WireGuard VPN only, so the app —
and the reminder's `GET /api/pacing/now` — work only while the phone's VPN is up;
a reminder that fires without it finds no server and stays silent.

## Two layers

**WebView** (`MainActivity`, on the fleet's shared `WebShellActivity`) — loads
`Config.BASE_URL`, navigation confined to the app + its NC login hop.

**Native geofence** — turning reminders on in the web app's Settings page records
your **home location on-device** (`Prefs` / SharedPreferences — never sent to the
server or committed to source) and arms a `GeofencingClient` geofence.
When you settle at home, `GeofenceBroadcastReceiver` calls `GET /api/pacing/now`
(reusing the WebView's session cookie) and posts a reminder **only if the backend
says `nudge`** — the backend already applies the window / night-cutoff / spacing
gates, so the phone stays a thin trigger. `BootReceiver` re-arms after a reboot.

Setup (`SetupFlow`) asks for fine location, captures home (on every run, so
"Update home" really moves it), then background location ("Allow all the time",
required for the geofence to fire while the app is closed), then notifications.

**The bridge is origin-scoped.** The Settings page drives that flow through
`window.CoachAndroid`, injected by `WebViewCompat.addWebMessageListener` with an
`allowedOriginRules` of exactly `Config.BASE_URL`. The library sheet embeds a
`youtube-nocookie.com` player, so the WebView runs somebody else's code;
`addJavascriptInterface` would expose the bridge to that frame too. `Bridge` also
checks `sourceOrigin` and `isMainFrame` on every message, as Android's guidance
recommends.

The contract is a `postMessage` out (`status`, `setup`, `disable`) and a `message`
event back carrying `{hasHome, armed}` — two booleans, never the coordinates. The
phone posts it again when the permission flow settles.

Without `WebViewFeature.WEB_MESSAGE_LISTENER` the bridge is absent and the
Settings page shows no reminders controls, as in a desktop browser.

Runs on any Android 8+ (minSdk 26).

## Build & install

Borrows the recall project's `android` nix dev shell (JDK 17 + Android SDK; the
Gradle wrapper pins Gradle). Install targets the Pixel 9 only (keys on device
model, not IP):

```sh
cd android
nix develop ~/Code/recall#android --command ./deploy.sh   # build + install to the Pixel 9
# or just build:
nix develop ~/Code/recall#android --command ./gradlew :app:assembleDebug
# → app/build/outputs/apk/debug/app-debug.apk
```

The APK is signed with the auto-generated debug key — fine for sideloading, the
only distribution path.

## Tests

```sh
cd android
nix develop ~/Code/recall#android --command ./gradlew :app:testDebugUnitTest
```

JVM unit tests — no device, no emulator. The parts of this app worth testing are
decisions, and all of them run on the JVM: who may use the bridge (`Bridge`),
the order of the setup flow (`SetupFlow`), which boundary crossing counts as
arriving home (`Geofencing.settledAtHome`), what a pacing response means
(`PacingClient.parse`), what the on-device home store does with a coordinate
(`Prefs`), and that the app is allowed to load itself (`Config`).
[Robolectric](https://robolectric.org) supplies a real `SharedPreferences` and a
real `org.json` — the android.jar stubs throw on every call — pinned to SDK 35 by
`app/src/test/resources/robolectric.properties` (it has no runtime for 36 yet).

The `gate.dhall` table runs this (plus `:app:assembleDebug`, since `MainActivity`
and the receivers carry no unit tests and packaging is what proves they still
build), so the pre-commit hook covers it — you don't have to remember. A missing
Android shell, or a missing `ui-harness` beside the repo, **fails** the gate
rather than skipping it.

Not in CI, and neither is any other Android app in the fleet: the GitHub runners
have no Android SDK and no sibling checkouts. The nightly `check --full` agent on
the Mac mini runs every repo's gate, which is where these get their
scheduled run.

## Layout

```
android/
├── app/src/main/
│   ├── AndroidManifest.xml                      # WebView activity + geofence/boot receivers
│   ├── kotlin/org/xinutec/coach/
│   │   ├── MainActivity.kt                       # WebView + reminders setup flow
│   │   ├── Bridge.kt · SetupFlow.kt              # who may ask what; which step is next
│   │   ├── Config.kt · Prefs.kt                  # constants + on-device home/armed state
│   │   ├── Geofencing.kt                         # arm/remove the home geofence
│   │   ├── GeofenceBroadcastReceiver.kt          # on home → query pacing → notify
│   │   ├── BootReceiver.kt                       # re-arm after reboot
│   │   └── PacingClient.kt · Notifications.kt    # authenticated GET + the reminder
│   └── res/                                      # launcher icon, theme, notification icon
├── build.gradle.kts · settings.gradle.kts · gradle/
└── deploy.sh                                     # build + adb install to the Pixel 9
```
