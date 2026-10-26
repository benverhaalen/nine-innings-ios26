# Agent guide: 9 Innings 2016 for iPhone

This repo builds an iPhone app that runs the original Android game *9 Innings: Pro Baseball 2016* (v6.0.7). The repo and the app contain **no game files**. On first launch the app downloads the original APK from a public mirror and verifies it by SHA-256 (`supported-apks.json`).

## Task: install the game on the user's iPhone

### If the user has a Mac (preferred)

1. `python3 scripts/build.py doctor`. Fix every `FIX` line:
   - **Xcode missing**: the user installs it from the App Store. It's large, so ask them to start it early.
   - **Rust missing**: `curl https://sh.rustup.rs -sSf | sh -s -- -y`, then `source ~/.cargo/env`.
   - **Rust iPhone target**: `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`.
   - **No Apple Development certificate**: the user opens Xcode → Settings → Accounts, adds their Apple ID, then *Manage Certificates → + → Apple Development*. You cannot do this for them.
   - **No iPhone**: the user connects it by cable, unlocks it and taps *Trust This Computer*. If prompted, they enable Developer Mode (Settings → Privacy & Security) and reboot.
2. `python3 scripts/build.py device`. Use `--device <id>` if several are connected (`xcrun devicectl list devices`).
3. Tell the user to:
   - trust the developer (Settings → General → VPN & Device Management)
   - open **9 Innings** and tap **Download game** (the app fetches and verifies the APK itself)
4. If the user already has the APK, pass `--apk /path/to/file.apk`. The build verifies the hash and bundles it, so no download is needed. Never commit or publish such a build.

A free Apple ID expires the app after 7 days. Rerun `build.py device` to refresh it; saves are kept.

### If there's no Mac

Send the user to the latest release's `NineInnings26.ipa` and the README's *Sideload the IPA* section (Impactor 2.7.2+ or Sideloadly), or to the SideStore/AltStore source URL. You can't sign for them without a Mac.

## Task: develop or optimize

- Simulator: boot one (`xcrun simctl boot <udid>`), then `python3 scripts/build.py simulator`.
  - Supply the APK once by copying it into the app's Documents folder (`xcrun simctl get_app_container booted io.github.nineinnings.ios26 data`), then relaunch. It's adopted and verified.
- Logs: `Documents/engine.log` in the app container. It includes fps, memory, offline decisions and unsupported calls, which are logged as `STOP …`.
- Test hooks, off unless the environment variable is set (prefix with `SIMCTL_CHILD_` for `simctl launch`):
  - `RDD_AUTOTEST_DOWNLOAD=1`: taps **Download game** on launch (the download itself is always automatic).
  - `RDD_AUTOTEST_TOUCHES=1`: the app runs `Documents/autotest-taps.txt` and then deletes it. Lines are `tap X Y`, `drag X1 Y1 X2 Y2` or `wait MS`, in the game's 800×480 coordinates.
- Run heavy builds one at a time; the engine itself needs about 250 MB.

## Rules

- Never add game content to the repo or to release assets: no APK, `libgame.so`, DEX, resources, extracted tables, art, or code translated from the game's bytecode. `build.py` audits IPAs built without `--apk`.
  - The one exception is the app icon. `build.py` downloads it at build time from `icon_sources` in `supported-apks.json` and checks its fingerprint, so it lands in built IPAs but never in the repo. There is no placeholder icon.
  - Anything game-specific is derived at runtime from the user's APK (`runtime/src/apk.rs`, `runtime/src/dalvik.rs`).
- Keep `supported-apks.json` hashes exact. Only add a mirror after verifying that its file's SHA-256 matches.
