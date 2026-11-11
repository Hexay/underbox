#!/usr/bin/env python3
"""Make a scratch copy of a game project start in one of its own scenes with our watcher in it.

Only ever pointed at a copy under /srv/agents/cache: the checkouts it comes from stay read-only.
Prints the copy's .sbproj path.
"""
import argparse
import json
import pathlib
import sys
import uuid

WATCH_NAME = "RealGameWatch"


def load(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def project_file(root):
    candidates = sorted(root.glob("*.sbproj")) + sorted(root.glob(".sbproj"))
    if not candidates:
        sys.exit(f"prepare: no .sbproj under {root}")
    return candidates[0]


def set_startup_scene(sbproj, scene):
    project = load(sbproj)
    meta = project.setdefault("Metadata", {})
    meta["DedicatedServerStartupScene"] = scene
    meta["StartupScene"] = scene
    meta.pop("MapStartupScene", None)
    # A listed map is a package the launcher asks for; the run must never request one.
    meta["MapList"] = []
    project["PackageReferences"] = []
    sbproj.write_text(json.dumps(project, indent=2), encoding="utf-8")


def add_watcher(scene_path, idle_seconds, command):
    scene = load(scene_path)
    objects = [g for g in scene["GameObjects"] if g.get("Name") != WATCH_NAME]
    objects.append({
        "__guid": str(uuid.uuid4()),
        "Flags": 0,
        "Name": WATCH_NAME,
        "Enabled": True,
        "Components": [{
            "__type": WATCH_NAME,
            "__guid": str(uuid.uuid4()),
            "IdleSeconds": idle_seconds,
            "StartCommand": command,
        }],
    })
    scene["GameObjects"] = objects
    scene_path.write_text(json.dumps(scene, indent=2), encoding="utf-8")


def main():
    args = argparse.ArgumentParser()
    args.add_argument("root", type=pathlib.Path)
    args.add_argument("scene", help="relative to Assets, e.g. scenes/zoo.scene; empty for the project's StartupScene")
    args.add_argument("watch", type=pathlib.Path)
    args.add_argument("--idle", type=int, default=15, help="seconds the watcher idles past the settle")
    args.add_argument("--command", default="", help="console command the host runs once things settle")
    a = args.parse_args()
    sbproj = project_file(a.root)
    scene = a.scene or load(sbproj).get("Metadata", {}).get("StartupScene") or ""
    scene_path = a.root / "Assets" / scene
    if not scene or not scene_path.is_file():
        sys.exit(f"prepare: no such scene {scene_path}")
    set_startup_scene(sbproj, scene)
    add_watcher(scene_path, a.idle, a.command)
    code = a.root / "Code"
    code.mkdir(exist_ok=True)
    (code / f"{WATCH_NAME}.cs").write_text(a.watch.read_text(encoding="utf-8"), encoding="utf-8")
    print(sbproj)


if __name__ == "__main__":
    main()
