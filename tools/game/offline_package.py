#!/usr/bin/env python3
"""Write the offline package record a joining client needs for a locally compiled game.

A client joining a dedicated server looks the announced game package up with Package.FetchAsync.
With the backend unreachable, managed falls back to what it remembered when it last installed the
package (Package.FetchOffline), then reads the revision's manifest from its ManifestUrl and finds
the listed files in its download cache (AssetDownloadCache). This writes, for the local project
only, a seed folder that tools/boot-client.sh copies over the client's game dir:

  .source2/cache/services/package.<org>.<ident>.json   PackageDto from the .sbproj, ManifestUrl = --manifest-url
  download/assets/_bin/package_<org>_<ident>.<crc>.cll  a placeholder code archive

and the manifest (one file, .bin/package.<org>.<ident>.cll) to --manifest-out, for a loopback HTTP
server to answer at --manifest-url. A cached manifest can't stand in for it: Utility.Web returns
null on a network error instead of throwing, so PackageRevision's offline fallback never runs.

The archive only satisfies "a game ships code". The game's real code reaches the client from
the server's CodeArchive table, which a dedicated server running a local project always sends
(GameInstanceDll.AddArchiveToCodeArchiveTable), and replaces the placeholder by assembly name.
"""
import argparse
import gzip
import json
import pathlib
import re
import struct
import sys
import zlib

ARCHIVE_VERSION = 1007
PLACEHOLDER = "// underbox offline package: the game's code comes from the server's CodeArchive table.\n" \
    "namespace Underbox.OfflinePackage { internal static class Placeholder { } }\n"


def load_project(root):
    candidates = sorted(root.glob("*.sbproj")) + sorted(root.glob(".sbproj"))
    if not candidates:
        sys.exit(f"offline_package: no .sbproj under {root}")
    return json.loads(candidates[0].read_text(encoding="utf-8-sig"))


def bs_string(text):
    data = text.encode("utf-8")
    return struct.pack("<i", len(data)) + data


def code_archive(compiler):
    """CodeArchive.Serialize: gzip of ByteStream strings (i32 length + UTF-8) and an i64 version."""
    config = {"RootNamespace": "Sandbox", "DefineConstants": "SANDBOX", "NoWarn": "1701;1702;1591;",
              "WarningsAsErrors": "", "AssemblyReferences": [], "IgnoreFolders": [],
              "ReplacementDirectives": {".Server.cs": "SERVER"}}
    trees = [{"Text": PLACEHOLDER, "LocalPath": "UnderboxOfflinePackage.cs"}]
    # An archive's references replace the compiler's defaults (Compiler.References.cs), so list them.
    references = ["Sandbox.System", "Sandbox.Engine", "Sandbox.Filesystem", "Sandbox.Reflection", "Sandbox.Mounting",
                  "Microsoft.AspNetCore.Components"]
    body = bs_string("GMCA") + struct.pack("<q", ARCHIVE_VERSION) + bs_string(compiler) \
        + bs_string(json.dumps(config)) + bs_string(json.dumps(references)) + bs_string(json.dumps(trees)) \
        + bs_string("[]") + bs_string("{}")
    return gzip.compress(body, mtime=0)


def cache_filename(path, crc):
    """AssetDownloadCache.CreateCacheFilename for an already-normalised (lowercase) path."""
    stem, ext = path.rsplit(".", 1)
    return f"{re.sub(r'[^a-zA-Z0-9/]', '_', stem).lstrip('/')}.{crc:x}.{ext}"


def package_dto(org, ident, title, kind, version_id, manifest_url):
    return {
        "Org": {"Ident": org, "Title": org},
        "Ident": ident,
        "Title": title,
        "Summary": "Local checkout, compiled by the server it joins",
        "TypeName": kind,
        "Public": False,
        "Tags": [],
        "PackageReferences": [],
        "EditorReferences": [],
        "Version": {"Id": version_id, "ManifestUrl": manifest_url, "FileCount": 1},
    }


def main():
    args = argparse.ArgumentParser()
    args.add_argument("root", type=pathlib.Path, help="the game project (a scratch copy)")
    args.add_argument("seed", type=pathlib.Path, help="folder laid out like the client's game dir")
    args.add_argument("--manifest-url", required=True, help="a loopback URL that serves --manifest-out")
    args.add_argument("--manifest-out", required=True, type=pathlib.Path)
    a = args.parse_args()

    project = load_project(a.root)
    org, ident = project["Org"].lower(), project["Ident"].lower()
    archive = code_archive(f"{org}.{ident}")
    crc = zlib.crc32(archive)
    path = f".bin/package.{org}.{ident}.cll"

    services = a.seed / ".source2/cache/services"
    services.mkdir(parents=True, exist_ok=True)
    # Any positive revision id: a game with one makes the client try (and fail) the backend's compiled dlls first.
    dto = package_dto(org, ident, project.get("Title") or ident, project.get("Type", "game"), crc or 1, a.manifest_url)
    (services / f"package.{org}.{ident}.json").write_text(json.dumps(dto), encoding="utf-8")
    # Url stays empty: the file is already in the download cache, so nothing is fetched for it.
    manifest = {"Schema": 1, "Files": [{"Url": "", "Crc": f"{crc:x}", "Path": path, "Size": len(archive)}]}
    a.manifest_out.parent.mkdir(parents=True, exist_ok=True)
    a.manifest_out.write_text(json.dumps(manifest), encoding="utf-8")

    cached = a.seed / "download/assets" / cache_filename(path, crc)
    cached.parent.mkdir(parents=True, exist_ok=True)
    cached.write_bytes(archive)
    print(f"{org}.{ident}")


if __name__ == "__main__":
    main()
