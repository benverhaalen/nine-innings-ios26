#!/usr/bin/env python3
"""Build, sign and install 9 Innings 2016 for iPhone.

    python3 scripts/build.py doctor
    python3 scripts/build.py ipa                    # dist/NineInnings26.ipa, ready for SideStore/AltStore/Sideloadly
    python3 scripts/build.py device   [--team ID]   # build, sign with your Apple ID, install on a connected iPhone
    python3 scripts/build.py simulator              # build, install and launch in the booted simulator

The app contains no game files. On first launch it fetches the original
9 Innings 2016 APK from a public mirror listed in supported-apks.json, verifies
its SHA-256, and then plays offline. Personal builds can skip that step:
    --apk PATH       bundle this APK (verified); do not publish such builds
    --apk-url URL    download an APK at build time (verified) and bundle it
"""
import argparse, hashlib, json, os, plistlib, re, shutil, subprocess, sys, tempfile, zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUNTIME = ROOT / "runtime"
DIST = ROOT / "dist"
BUILD = ROOT / "build"
SUPPORTED = json.loads((ROOT / "supported-apks.json").read_text())["apks"]
APP_NAME = "NineInnings"
VERSION = (ROOT / "VERSION").read_text().strip()


def run(cmd, **kw):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    return subprocess.run(cmd, check=True, **kw)


def out(cmd):
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def check_apk(path):
    path = Path(path).expanduser()
    if not path.is_file():
        sys.exit(f"APK not found: {path}")
    digest = sha256(path)
    match = next((a for a in SUPPORTED if a["sha256"] == digest), None)
    if not match:
        sys.exit(f"{path.name} is not a supported APK.\n  sha256 {digest}\n  needed: " +
                 ", ".join(f"{a['title']} {a['version']} ({a['sha256'][:16]}…)" for a in SUPPORTED))
    print(f"APK ok: {match['title']} {match['version']}")
    return path


def resolve_apk(a):
    """The APK to bundle into a personal build, or None (the default) to fetch on first launch."""
    if a.apk_url:
        import urllib.request
        target = BUILD / "downloads" / "original.apk"
        target.parent.mkdir(parents=True, exist_ok=True)
        print(f"downloading {a.apk_url}")
        urllib.request.urlretrieve(a.apk_url, target)
        return check_apk(target)
    if a.apk:
        return check_apk(a.apk)
    return None


def cargo(target):
    env = dict(os.environ, IPHONEOS_DEPLOYMENT_TARGET="15.0")
    run(["cargo", "build", "--release", "--target", target, "--bin", "engine-startup"], cwd=RUNTIME, env=env)
    return RUNTIME / "target" / target / "release" / "engine-startup"


def info_plist(bundle_id, platform):
    return {
        "CFBundleExecutable": APP_NAME, "CFBundleIdentifier": bundle_id,
        "CFBundleName": "9 Innings", "CFBundleDisplayName": "9 Innings",
        "CFBundleVersion": VERSION, "CFBundleShortVersionString": VERSION,
        "CFBundlePackageType": "APPL", "CFBundleSupportedPlatforms": [platform],
        "CFBundleInfoDictionaryVersion": "6.0", "CFBundleDevelopmentRegion": "en",
        "MinimumOSVersion": "15.0", "UIDeviceFamily": [1, 2], "UIRequiredDeviceCapabilities": ["arm64"],
        "UISupportedInterfaceOrientations": ["UIInterfaceOrientationLandscapeLeft", "UIInterfaceOrientationLandscapeRight"],
        "UISupportedInterfaceOrientations~ipad": ["UIInterfaceOrientationLandscapeLeft", "UIInterfaceOrientationLandscapeRight"],
        "UIRequiresFullScreen": True, "UIStatusBarHidden": True, "UILaunchScreen": {},
        "CFBundleIcons": {"CFBundlePrimaryIcon": {"CFBundleIconFiles": ["AppIcon60x60"], "CFBundleIconName": "AppIcon"}},
        "CFBundleIcons~ipad": {"CFBundlePrimaryIcon": {"CFBundleIconFiles": ["AppIcon60x60", "AppIcon76x76"], "CFBundleIconName": "AppIcon"}},
        # Let players hand the APK to the app from Files, the share sheet, AirDrop or Finder.
        "UIFileSharingEnabled": True, "LSSupportsOpeningDocumentsInPlace": False,
        "UTImportedTypeDeclarations": [{
            "UTTypeIdentifier": "com.android.package-archive", "UTTypeDescription": "Android package",
            "UTTypeConformsTo": ["public.data", "public.archive"],
            "UTTypeTagSpecification": {"public.filename-extension": ["apk"], "public.mime-type": ["application/vnd.android.package-archive"]}}],
        "CFBundleDocumentTypes": [{"CFBundleTypeName": "Android package", "LSHandlerRank": "Alternate",
                                   "LSItemContentTypes": ["com.android.package-archive"]}],
    }


def apk_icon(apk, out):
    """The game's own launcher icon, read from the user's APK at build time (never stored in the repo).
    The APK's largest icon is 96 px with a soft 2 px edge; crop the edge and let sips scale it to each iOS size."""
    with zipfile.ZipFile(apk) as z:
        name = max((n for n in z.namelist() if re.fullmatch(r"res/drawable-\w*dpi/icon\.png", n)), key=lambda n: z.getinfo(n).file_size)
        src = out / "apk-icon.png"
        src.write_bytes(z.read(name))
    return crop_apk_icon(src)


def crop_apk_icon(src):
    run(["sips", "-c", "92", "92", str(src)], capture_output=True)
    return src


def installed_icon(fetch, out):
    """The icon the app saved from the player's APK (Documents/runtime-data/icon.png) on an earlier install,
    so a rebuild shows the game's icon without the APK or any art living on this Mac or in the repo."""
    src = out / "apk-icon.png"
    src.unlink(missing_ok=True)
    try:
        fetch(src)
    except (subprocess.CalledProcessError, OSError):
        return None
    if not src.exists():
        return None
    print("Using the game icon saved on the device from your APK.")
    return crop_apk_icon(src)


def icon_fingerprint(png):
    """64-bit average hash: brighter-than-mean bits of the image scaled to 8×8 (via sips, so no imaging library is needed)."""
    with tempfile.TemporaryDirectory() as tmp:
        bmp = Path(tmp) / "icon.bmp"
        subprocess.run(["sips", "-s", "format", "bmp", "-z", "8", "8", str(png), "--out", str(bmp)], check=True, capture_output=True)
        b = bmp.read_bytes()
    offset, width, height, bpp = int.from_bytes(b[10:14], "little"), int.from_bytes(b[18:22], "little", signed=True), int.from_bytes(b[22:26], "little", signed=True), int.from_bytes(b[28:30], "little")
    row = (width * bpp // 8 + 3) & ~3
    gray = [sum(b[offset + y * row + x * bpp // 8 + c] for c in range(3)) for y in range(abs(height)) for x in range(width)]
    mean = sum(gray) / len(gray)
    return sum(1 << i for i, v in enumerate(gray) if v > mean)


def store_icon(out):
    """The game's 512 px store icon, downloaded at build time from a public image host (never stored in this repo).
    Hosts re-encode the image on each request, so it's checked by its 8×8 fingerprint rather than by SHA-256:
    the store icon and the APK's own icon match within a bit or two, unrelated art differs by 20+."""
    for source in (s for apk in SUPPORTED for s in apk.get("icon_sources", [])):
        dest = out / "store-icon.png"
        try:
            run(["curl", "-fsSL", "--max-time", "30", "-o", str(dest), source["url"]], capture_output=True)
            distance = bin(icon_fingerprint(dest) ^ int(source["fingerprint"], 16)).count("1")
        except (subprocess.CalledProcessError, OSError, ValueError):
            continue
        if distance <= 6:
            print(f"Using the game icon from {source['name']}.")
            # Drop the alpha channel (its slightly rounded corners fall inside the iOS icon mask anyway).
            flat = out / "store-icon.jpg"
            run(["sips", "-s", "format", "jpeg", "-s", "formatOptions", "100", str(dest), "--out", str(flat)], capture_output=True)
            return flat
    return None


def assemble(executable, bundle_id, platform, apk=None, icon_apk=None, icon=None, saved_icon=None, require_icon=False):
    app = BUILD / platform / f"{APP_NAME}.app"
    shutil.rmtree(app, ignore_errors=True)
    app.mkdir(parents=True)
    shutil.copy2(executable, app / APP_NAME)
    shutil.copytree(ROOT / "resources" / "fonts", app / "fonts", ignore=shutil.ignore_patterns("NOTICE"))
    # Icon: --icon, else the store icon, else the APK's own (from --apk/--icon-from or saved on the device by an earlier install).
    original = (Path(icon).expanduser() if icon else store_icon(app.parent)
                or (apk_icon(icon_apk or apk, app.parent) if (icon_apk or apk) else saved_icon and installed_icon(saved_icon, app.parent)))
    if not original and require_icon:
        sys.exit("Couldn't download the game icon from any icon_sources entry in supported-apks.json; not building a release without it.")
    if not original:
        print("warning: no game icon available (icon host unreachable); this build has no icon until the next rebuild.")
    for name, size in [("AppIcon60x60@2x.png", 120), ("AppIcon60x60@3x.png", 180), ("AppIcon76x76@2x~ipad.png", 152), ("AppIcon76x76~ipad.png", 76)] if original else []:
        run(["sips", "-s", "format", "png", "-z", str(size), str(size), str(original), "--out", str(app / name)], capture_output=True)
    (app / "Info.plist").write_bytes(plistlib.dumps(info_plist(bundle_id, platform)))
    (app / "PkgInfo").write_text("APPL????")
    (app / "supported-apks.json").write_text((ROOT / "supported-apks.json").read_text())
    if apk:
        shutil.copy2(apk, app / "original.apk")  # adopted into Documents on first launch
    else:
        audit(app)
    return app


def audit(app):
    """Refuse to produce an app that contains anything from the game."""
    banned = {a["sha256"] for a in SUPPORTED}
    for f in app.rglob("*"):
        if f.is_file():
            if f.suffix.lower() in {".apk", ".so", ".dex", ".arsc"} or sha256(f) in banned:
                sys.exit(f"audit failed: {f} looks like game content")
            if f.read_bytes()[:4] == b"PK\x03\x04":
                sys.exit(f"audit failed: unexpected archive {f}")
    print(f"audit ok: {app.name} contains no game files")


def default_team():
    """Team ID of the first Apple Development certificate in the keychain."""
    pem = subprocess.run(["security", "find-certificate", "-a", "-c", "Apple Development", "-p"], capture_output=True, text=True).stdout
    for cert in re.findall(r"-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----", pem, re.S):
        subject = subprocess.run(["openssl", "x509", "-noout", "-subject"], input=cert, capture_output=True, text=True).stdout
        m = re.search(r"OU\s*=\s*([A-Z0-9]{10})", subject)
        if m:
            return m.group(1)
    return None


def signing_identity(team):
    for line in out(["security", "find-identity", "-v", "-p", "codesigning"]).splitlines():
        m = re.search(r'\s([0-9A-F]{40})\s+"(Apple Development: [^"]+)"', line)
        if m:
            pem = subprocess.run(["security", "find-certificate", "-c", m.group(2), "-p"], capture_output=True, text=True).stdout
            subject = subprocess.run(["openssl", "x509", "-noout", "-subject"], input=pem, capture_output=True, text=True).stdout
            if f"OU={team}" in subject.replace(" ", ""):
                return m.group(1)
    sys.exit(f"No 'Apple Development' signing certificate for team {team}. Open Xcode → Settings → Accounts, add your Apple ID, then 'Manage Certificates' → + → Apple Development.")


def provision(team, bundle_id, udid):
    """Let Xcode create/download a development profile for bundle_id using a tiny stub app.
    Building for the phone's UDID makes Xcode register it, so the profile includes that phone."""
    derived = BUILD / "signing-stub"
    run(["xcodebuild", "-project", str(ROOT / "scripts/signing-stub/SigningStub.xcodeproj"), "-scheme", "SigningStub", "-configuration", "Release",
         "-sdk", "iphoneos", "-destination", f"id={udid}", "-derivedDataPath", str(derived), "-allowProvisioningUpdates", "-allowProvisioningDeviceRegistration",
         f"DEVELOPMENT_TEAM={team}", f"PRODUCT_BUNDLE_IDENTIFIER={bundle_id}", "build"])
    stub = derived / "Build/Products/Release-iphoneos/SigningStub.app"
    xcent = next(derived.rglob("SigningStub.app.xcent"))
    return stub / "embedded.mobileprovision", xcent


def devices():
    with tempfile.NamedTemporaryFile(suffix=".json") as f:
        subprocess.run(["xcrun", "devicectl", "list", "devices", "--json-output", f.name], capture_output=True)
        data = json.loads(Path(f.name).read_text() or "{}")
    found = [d for d in data.get("result", {}).get("devices", []) if d.get("hardwareProperties", {}).get("platform") == "iOS"]
    # Paired phones that are out of reach are listed as "unavailable"; never pick one over a reachable phone.
    return sorted(found, key=lambda d: d.get("connectionProperties", {}).get("tunnelState") == "unavailable")


def cmd_ipa(a):
    apk = resolve_apk(a)
    app = assemble(cargo("aarch64-apple-ios"), a.bundle_id or "io.github.nineinnings.ios26", "iPhoneOS", apk, require_icon=True)
    run(["codesign", "--force", "--sign", "-", "--timestamp=none", str(app)])  # ad-hoc; sideloading tools re-sign
    DIST.mkdir(exist_ok=True)
    ipa = DIST / ("NineInnings26-personal.ipa" if apk else "NineInnings26.ipa")
    ipa.unlink(missing_ok=True)
    with zipfile.ZipFile(ipa, "w", zipfile.ZIP_DEFLATED) as z:
        for f in sorted(app.rglob("*")):
            info = zipfile.ZipInfo.from_file(f, Path("Payload") / f.relative_to(app.parent))
            if f.is_file():
                info.compress_type = zipfile.ZIP_DEFLATED
                z.writestr(info, f.read_bytes())
            else:
                z.writestr(info, b"")
    (DIST / "NineInnings26.ipa.sha256").write_text(f"{sha256(ipa)}  {ipa.name}\n")
    print(f"\n{ipa} ({ipa.stat().st_size / 1e6:.1f} MB){' — no game files bundled' if not apk else ''}. Install it with SideStore, AltStore, Sideloadly or Impactor.")


def cmd_device(a):
    apk = resolve_apk(a)
    team = a.team or default_team()
    if not team:
        sys.exit("No Apple Development certificate found. Open Xcode once, sign in with your Apple ID (Settings → Accounts), then rerun, or pass --team.")
    bundle_id = a.bundle_id or f"io.github.nineinnings.ios26.{team.lower()}"
    found = devices()
    chosen = next((d for d in found if a.device in (d["identifier"], d["hardwareProperties"].get("udid"))), None) if a.device else (found[0] if found else None)
    if not chosen:
        sys.exit("No iPhone found. Connect it with a cable, unlock it and tap 'Trust This Computer'.")
    saved_icon = lambda dest: run(["xcrun", "devicectl", "device", "copy", "from", "--device", chosen["identifier"], "--domain-type", "appDataContainer",
                                   "--domain-identifier", bundle_id, "--source", "Documents/runtime-data/icon.png", "--destination", str(dest)], capture_output=True)
    app = assemble(cargo("aarch64-apple-ios"), bundle_id, "iPhoneOS", apk, a.icon_from and check_apk(a.icon_from), a.icon, saved_icon)
    profile, entitlements = provision(team, bundle_id, chosen["hardwareProperties"]["udid"])
    shutil.copy2(profile, app / "embedded.mobileprovision")
    run(["codesign", "--force", "--sign", signing_identity(team), "--entitlements", str(entitlements), "--timestamp=none", "--generate-entitlement-der", str(app)])
    run(["xcrun", "devicectl", "device", "install", "app", "--device", chosen["identifier"], str(app)])
    print("\nInstalled. On the iPhone: Settings → General → VPN & Device Management → trust your Apple ID, "
          "and if asked turn on Settings → Privacy & Security → Developer Mode. Then open 9 Innings.")
    if not apk:
        print("On first launch tap 'Download game' (one time, about 58 MB); after that it plays offline.")
    print("Free Apple IDs: the app stops opening after 7 days; rerun this command to refresh it (your save is kept).")


def cmd_simulator(a):
    apk = resolve_apk(a)
    bundle_id = a.bundle_id or "io.github.nineinnings.ios26"
    def saved_icon(dest):
        container = out(["xcrun", "simctl", "get_app_container", "booted", bundle_id, "data"])
        shutil.copy2(Path(container.strip()) / "Documents/runtime-data/icon.png", dest)
    app = assemble(cargo("aarch64-apple-ios-sim"), bundle_id, "iPhoneSimulator", apk, a.icon_from and check_apk(a.icon_from), a.icon, saved_icon)
    run(["codesign", "--force", "--sign", "-", str(app)])
    run(["xcrun", "simctl", "install", "booted", str(app)])
    run(["xcrun", "simctl", "launch", "booted", bundle_id])


def cmd_doctor(a):
    ok = True
    def line(good, text):
        nonlocal ok
        ok &= good
        print(("  ok   " if good else "  FIX  ") + text)
    print("9 Innings for iPhone — environment check")
    line(sys.platform == "darwin", "macOS (needed to build; Windows/Linux users install the prebuilt IPA instead)")
    line(shutil.which("xcodebuild") is not None, "Xcode installed (App Store, free)")
    line(shutil.which("cargo") is not None, "Rust installed (https://rustup.rs)")
    targets = subprocess.run(["rustup", "target", "list", "--installed"], capture_output=True, text=True).stdout if shutil.which("rustup") else ""
    line("aarch64-apple-ios" in targets.split(), "Rust iPhone target (rustup target add aarch64-apple-ios aarch64-apple-ios-sim)")
    team = default_team() if sys.platform == "darwin" else None
    line(team is not None, f"Apple Development certificate{' (team ' + team + ')' if team else ' — sign in to Xcode with your Apple ID'}")
    found = devices() if sys.platform == "darwin" else []
    line(bool(found), f"iPhone connected{': ' + found[0].get('deviceProperties', {}).get('name', '?') if found else ' — plug in, unlock, Trust This Computer'}")
    if found:
        line(found[0].get("deviceProperties", {}).get("developerModeStatus") != "disabled",
             "Developer Mode on the iPhone (Settings → Privacy & Security → Developer Mode → On, then restart the phone)")
    print("\nReady." if ok else "\nFix the items marked FIX, then rerun.")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("command", choices=["doctor", "ipa", "device", "simulator"])
    p.add_argument("--apk", help="personal builds: bundle this APK (verified) so no download is needed")
    p.add_argument("--apk-url", help="download the APK from this URL (hash-verified) instead")
    p.add_argument("--icon-from", help="personal builds: use the original game icon from this APK (verified; the APK itself isn't bundled)")
    p.add_argument("--icon", help="personal builds: use this square PNG as the app icon (e.g. a higher-resolution copy of the original)")
    p.add_argument("--team", help="Apple developer team ID (default: from your Apple Development certificate)")
    p.add_argument("--bundle-id", help="override the app's bundle identifier")
    p.add_argument("--device", help="device identifier from `xcrun devicectl list devices`")
    a = p.parse_args()
    {"doctor": cmd_doctor, "ipa": cmd_ipa, "device": cmd_device, "simulator": cmd_simulator}[a.command](a)


if __name__ == "__main__":
    main()
