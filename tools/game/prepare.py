#!/usr/bin/env python3
"""Make a scratch copy of a game project start in one of its own scenes with our watcher in it.

Only ever pointed at a copy under $UNDERBOX_GAME_COPIES: the checkouts it comes from stay read-only.
Prints the copy's .sbproj path.
"""
import argparse
import json
import pathlib
import re
import sys
import uuid


def load(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def project_file(root):
    candidates = sorted(root.glob("*.sbproj")) + sorted(root.glob(".sbproj"))
    if not candidates:
        sys.exit(f"prepare: no .sbproj under {root}")
    # A project template's `$ident.sbproj` isn't found by its ident; a plain `.sbproj` is.
    if "$" in candidates[0].name:
        return candidates[0].rename(root / ".sbproj")
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
    # Templates carry their folder name (game.minimal): a dot in the ident splits `org.ident` wrong.
    project["Ident"] = project.get("Ident", "").replace(".", "_")
    sbproj.write_text(json.dumps(project, indent=2), encoding="utf-8")


def on_disk(root, rel):
    """`root/rel` as spelled on disk; engine2 and managed resolve resource paths case-blind (walker: Scenes/)."""
    path = root
    for part in pathlib.PurePosixPath(rel).parts:
        exact = path / part
        if not exact.exists() and path.is_dir():
            exact = next((p for p in path.iterdir() if p.name.lower() == part.lower()), exact)
        path = exact
    return path


def add_watcher(scene_path, name, props):
    scene = load(scene_path)
    objects = [g for g in scene["GameObjects"] if g.get("Name") != name]
    objects.append({
        "__guid": str(uuid.uuid4()),
        "Flags": 0,
        "Name": name,
        "Enabled": True,
        "Components": [{"__type": name, "__guid": str(uuid.uuid4()), **props}],
    })
    scene["GameObjects"] = objects
    scene_path.write_text(json.dumps(scene, indent=2), encoding="utf-8")


def stub_lobby(code, name):
    """Route the game's `Networking.CreateLobby(...)` to `<name>.SkipLobby(...)`: real engine2 crashes
    in libsteam_api creating a lobby under -nosteam (docs/differential-render.md, walker)."""
    for path in code.rglob("*.cs"):
        text = path.read_text(encoding="utf-8-sig")
        stubbed = re.sub(r"\bNetworking\.CreateLobby\s*\(", f"{name}.SkipLobby(", text)
        if stubbed != text:
            path.write_text(stubbed, encoding="utf-8")


def prop(text):
    """`Key=value`: value as JSON when it parses (15, true), else as a string ("0,0,64")."""
    key, _, value = text.partition("=")
    try:
        return key, json.loads(value)
    except ValueError:
        return key, value


def main():
    args = argparse.ArgumentParser()
    args.add_argument("root", type=pathlib.Path)
    args.add_argument("scene", help="relative to Assets, e.g. scenes/zoo.scene; empty for the project's StartupScene")
    args.add_argument("watch", type=pathlib.Path, help="the component's .cs; its stem names the component")
    args.add_argument("--prop", type=prop, action="append", default=[], help="Key=value set on the injected component")
    args.add_argument("--stub-lobby", action="store_true", help="the component's static SkipLobby replaces Networking.CreateLobby")
    a = args.parse_args()
    name = a.watch.stem
    sbproj = project_file(a.root)
    scene = a.scene or load(sbproj).get("Metadata", {}).get("StartupScene") or ""
    scene_path = on_disk(a.root / "Assets", scene)
    if not scene or not scene_path.is_file():
        sys.exit(f"prepare: no such scene {scene_path}")
    set_startup_scene(sbproj, scene)
    add_watcher(scene_path, name, dict(a.prop))
    code = a.root / "Code"
    code.mkdir(exist_ok=True)
    if a.stub_lobby:
        stub_lobby(code, name)
    (code / f"{name}.cs").write_text(a.watch.read_text(encoding="utf-8"), encoding="utf-8")
    print(sbproj)


if __name__ == "__main__":
    main()
