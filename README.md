# 9 Innings 2016 for iPhone

**The original *9 Innings: Pro Baseball 2016* (Com2uS, Android v6.0.7), running on iPhone.**
It isn't a remake: the app runs the game's own engine in an ARM interpreter, so menus, stadiums, pitching and batting look and play exactly like 2016.

| | |
|---|---|
| ![Title](docs/screenshots/title.png) | ![Main menu](docs/screenshots/main-menu.png) |
| ![At bat](docs/screenshots/at-bat.png) | ![Pitch](docs/screenshots/pitch.png) |
| ![Ball call](docs/screenshots/ball-call.png) | ![Inning result](docs/screenshots/inning-result.png) |

*Real screenshots from this app (iOS simulator), not promotional art.*

> **No game files are in this repo or in the app download.** On first launch the app opens a public mirror of the original APK. You tap the site's Download button and the app checks the file's SHA-256 against `supported-apks.json`. From then on it plays offline.
>
> ![First launch](docs/screenshots/first-launch.png)

## Install

Pick the first row that fits you.

| You have… | Do this | Time |
|---|---|---|
| **A Mac** (or Claude/Codex running on one) | [Mac: one command](#mac-one-command) | ~10 min |
| **Windows / Linux / a Mac without Xcode** | [Sideload the IPA](#sideload-the-ipa) with Impactor or Sideloadly | ~5 min |
| **SideStore or AltStore** already on the iPhone | [Add the source](#sidestore--altstore) | 1 min |

Every option uses a free Apple ID, and Apple signs free-ID apps for **7 days** at a time. How to keep it working:

- **SideStore (recommended, set-and-forget):** it re-signs the app in the background on the phone, without a computer, so in practice it never expires. Open SideStore now and then, or use a Shortcuts automation, so the refresh runs.
- **Mac / Impactor / Sideloadly:** reinstall once a week. Your save is kept.
- **Paid Apple developer account ($99/yr):** signatures last a year.

TrollStore installs never expire, but TrollStore only works on old iOS versions (up to 17.0), not on current iPhones.

### Mac: one command

```sh
git clone https://github.com/benverhaalen/nine-innings-ios26 && cd nine-innings-ios26
python3 scripts/build.py doctor     # checks Xcode, Rust, your Apple ID and the iPhone, and tells you what to fix
python3 scripts/build.py device     # builds, signs with your Apple ID and installs on the connected iPhone
```

Optional: `--icon-from /path/to/the.apk` uses the game's original icon from your APK, or `--icon icon.png` uses any square PNG. The icon only goes into your own build, never into this repo.

First time only: install Xcode (App Store) and Rust (`curl https://sh.rustup.rs -sSf | sh`, then `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`). Sign in to Xcode with your Apple ID (Settings → Accounts). Connect the iPhone by cable, unlock it and tap *Trust*.

On the iPhone, trust your Apple ID (Settings → General → VPN & Device Management) and enable Developer Mode if asked. Then open **9 Innings** and tap **Download game**.

> **Using an AI agent?** Tell it: *"Install the game from this repo on my iPhone."* [AGENTS.md](AGENTS.md) has the exact steps.

### Sideload the IPA

1. Download **`NineInnings26.ipa`** from [Releases](https://github.com/benverhaalen/nine-innings-ios26/releases/latest).
2. Install it with one of these, signing in with a free Apple ID:
   - [Impactor](https://github.com/khcrysalis/PlumeImpactor) **2.7.2 or newer** (Windows/macOS/Linux)
   - [Sideloadly](https://sideloadly.io) (Windows/macOS)
3. Trust the profile and enable Developer Mode as above, then open the app and tap **Download game**.

### SideStore / AltStore

Add this source: `https://github.com/benverhaalen/nine-innings-ios26/releases/latest/download/apps.json`, then install **9 Innings 2016**. Updates appear automatically.
Don't have SideStore yet? [iloader](https://github.com/nab138/iloader) (2.4.0 or newer) sets it up from a computer in a few clicks.

## First launch

- **Download game** opens APKMirror inside the app. Tap the site's download button; the game installs itself when the download finishes (about 58 MB, once).
- **I already have the APK file** lets you pick a `.apk` from Files. You can also AirDrop or share it to the app, or copy it into the app's folder with Finder (iPhone → Files → 9 Innings).
- Only the exact file is accepted: v6.0.7, SHA-256 `50b1cd98…df9221`.

## What works

- Offline play: setup, team selection, main menu, game modes and Friendly-match gameplay are verified (screenshots above). League, Special and Home Run modes use the same engine and are expected to work, but haven't been checked one by one yet. Saves persist on the device.
- The "Disconnected" and "Not connected to the server" notices are normal: Com2uS shut the servers down. Tap **OK** and keep playing.
- Online features (Card Market, mailbox, events, rankings) are offline for now. Restoring them with a replacement server is in progress.
- Performance: the engine runs at its native 20 updates per second (its own 50 ms timer).
- Not implemented yet: sound.

## Troubleshooting

| Message / symptom | Fix |
|---|---|
| "Unable to Verify App" / the app won't open | Settings → General → VPN & Device Management → trust your Apple ID. Developer Mode must be on (Settings → Privacy & Security). |
| The app stopped opening after a week | The free 7-day signature expired. Run `build.py device` again, or refresh in SideStore/AltStore. Your save stays. |
| Sideloading tool rejects your Apple ID login (since Oct 2026) | Update to Impactor 2.7.2+ / iloader 2.4.0+, or use the Mac route, which signs through Xcode. |
| "This isn't the supported APK" | The file isn't v6.0.7 (`com.com2us.nipb2013…`, 60,322,552 bytes). Use **Download game**, or get that exact file. |
| Download page looks different or the button is missing | Mirrors change their pages. Scroll to the APK download button. If the mirror is gone, use **I already have the APK file** and [open an issue](../../issues). |
| `build.py doctor` shows FIX lines | Do what each line says, then rerun. |

## For developers

```
runtime/ios-graphics.m       UIKit host: rendering, touch, text, first-launch download/import
runtime/src/bin/engine-startup.rs   loads libgame.so from the APK and runs it; fake JNI/Android
runtime/src/apk.rs           reads assets, JNI method list and string resources from the APK at launch
runtime/src/dalvik.rs        tiny Dalvik interpreter for the game's two Java text-layout methods
vendor/touchhle-arm-interpreter   ARMv7 interpreter (from touchHLE, MPL-2.0)
scripts/build.py             doctor / ipa / device / simulator
```

`python3 scripts/build.py simulator` runs it in the iOS Simulator. [AGENTS.md](AGENTS.md) covers the test hooks for scripted taps and screenshots.

## Legal

*9 Innings* and all of its content belong to Com2uS. This project contains none of it. It is an independent compatibility layer that runs a copy of the game you download yourself. It is not affiliated with or endorsed by Com2uS, MLB or the MLBPA. Code: MIT ([LICENSE](LICENSE)); third-party components: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
