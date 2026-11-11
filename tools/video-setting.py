"""Set one of managed's stored video settings (config/video.json in a game dir) before a boot.

usage: python3 -I tools/video-setting.py <game-dir> <key> <value>
e.g. `shadow.quality 2` (ShadowQuality.High). Managed applies the stored quality profiles at startup
(RenderSettings.ApplyQualityProfiles), after command-line convars, so a golden that depends on them
pins them here. Also marks the first-run preset as done so it can't overwrite the value.
"""
import json
import pathlib
import sys

game, key, value = sys.argv[1:4]
path = pathlib.Path(game) / "config" / "video.json"
settings = json.loads(path.read_text()) if path.exists() else {}
for k, v in ((key, value), ("quality.autodetected", "true")):
    settings[k] = {"Value": v, "Timeout": -1, "DeleteAt": 0}
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(settings, indent=2))
