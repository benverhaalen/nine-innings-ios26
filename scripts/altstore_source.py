#!/usr/bin/env python3
"""Write the SideStore/AltStore source (apps.json) for a release IPA.

    python3 scripts/altstore_source.py dist/NineInnings26.ipa > dist/apps.json
"""
import datetime, json, sys
from pathlib import Path

REPO = "benverhaalen/nine-innings-ios26"
RAW = f"https://raw.githubusercontent.com/{REPO}/main"
ipa = Path(sys.argv[1])
root = Path(__file__).resolve().parent.parent
version = (root / "VERSION").read_text().strip()
icon = json.loads((root / "supported-apks.json").read_text())["apks"][0]["icon_sources"][0]["url"]
print(json.dumps({
    "name": "9 Innings 2016 for iPhone",
    "identifier": "io.github.nineinnings.ios26.source",
    "sourceURL": f"https://github.com/{REPO}/releases/latest/download/apps.json",
    "website": f"https://github.com/{REPO}",
    "apps": [{
        "name": "9 Innings 2016",
        "bundleIdentifier": "io.github.nineinnings.ios26",
        "developerName": "nine-innings-ios26",
        "subtitle": "The original 2016 baseball game, on iPhone.",
        "localizedDescription": "Runs the original Android game 9 Innings: Pro Baseball 2016 (v6.0.7). "
                                "On first launch it downloads the game from a public mirror and verifies it; then it plays offline. "
                                "Contains no game files apart from its icon. Not affiliated with Com2uS.",
        "iconURL": icon,
        "tintColor": "0D1729",
        "screenshotURLs": [f"{RAW}/docs/screenshots/{n}.png" for n in ["title", "main-menu", "at-bat", "pitch"]],
        "versions": [{
            "version": version,
            "date": datetime.date.today().isoformat(),
            "downloadURL": f"https://github.com/{REPO}/releases/download/v{version}/{ipa.name}",
            "size": ipa.stat().st_size,
            "minOSVersion": "15.0",
        }],
        "appPermissions": {"entitlements": [], "privacy": {}},
    }],
    "news": [],
}, indent=2))
