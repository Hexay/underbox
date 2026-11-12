"""Which textures each captured draw uses (docs/differential-gpu.md, "Textures"), for gpu.py and gpu_diff.py.

A draw's textures are those bound to it directly plus the bindless slots its constants select (FIELDS).
Images are named by their debug label when the engine gave one (ours labels textures by resource path),
else by matching their uploaded mip 0 against the seeded core's `.vtex_c` (gpu_vtex.py; real uploads
them as stored).
"""
import re
import struct
from collections import Counter, defaultdict

import gpu_vtex

# Bindless indices in a draw's constants, found with `gpu.py fields` on showroom and the public structs:
# (engine, source, element stride, byte offset, 16-bit half, what). "ub<size>" is a uniform buffer of that
# size; "sb<count>x<stride>" a storage buffer of count elements (managed's capacities; ours' copies carry 4
# extra bytes); "s<set>b<binding>" whatever storage buffer sits there (ours sizes its decal buffer to the
# decal count). Ours' copies of the shadow-map indices select default 1x1 slots: its stand-in binds shadow
# maps directly, so they apply to real only. Both read the contact-shadow mask by index.
DECAL = [(".lo", 40, "decal color"), (".lo", 44, "decal rma")]  # GPUDecal words 10 and 11
FIELDS = [
    ("real", "ub416", 0, 288 + 4 * k, "", f"sun cascade {k}") for k in range(4)
] + [
    (side, "ub416", 0, 312, "", "contact shadows")  # DirectionalLightCB.g_DirectionalLightScreenSpaceShadowIndex
    for side in ("real", "ours")
] + [
    ("real", "sb512x76", 76, 64, "", "spot shadow"),  # ProjectedShadows[].ShadowMapTextureIndex
    ("real", "sb256x408", 408, 396, "", "point shadow"),  # ProjectedCubeShadows[].ShadowMapTextureCubeIndex
] + [("real", "sb2048x64", 64, off, half, what) for half, off, what in DECAL] + [
    ("ours", "s0b40", 64, off, half, what) for half, off, what in DECAL]  # the stand-in's DecalsBuffer
SENTINELS = (0, 0xFFFF, 0xFFFFFFFF)
STORAGE_SCAN = 64 * 1024  # bytes of a storage buffer `gpu.py fields` searches

TEXTURE_NAME = re.compile(r"\.(tga|png|jpg|jpeg|psd|tif|tiff|exr|hdr|pfm|vtex)$", re.I)


def images(fr):
    """Every image description in the frame: directly bound views and bindless slots."""
    for d in fr.draws() + [x for k, x in fr.ops if k in ("dispatch", "draw")]:
        for r in d.get("res") or []:
            for e in r.get("el", []):
                if (e.get("view") or {}).get("img"):
                    yield e["view"]["img"]
    for b in fr.bindless.values():
        for x in b["index"].values():
            if x.get("img"):
                yield x["img"]


def slots(fr):
    """{index: image} over the frame's bindless image arrays. Ours has one array per view dimension under a
    shared numbering; every index FIELDS decodes is sampled as a Texture2D, so 2D views win."""
    if not hasattr(fr, "slot_index"):
        views = [(i, x) for b in fr.bindless.values() for i, x in b["index"].items() if x.get("img")]
        fr.slot_index = {i: x["img"] for i, x in sorted(views, key=lambda v: v[1].get("type") == "2D")}
    return fr.slot_index


def buffers(fr, d):
    """(source, binding, bytes) for a draw's uniform buffers, storage buffers and push constants."""
    for r in d.get("res") or []:
        for e in r.get("el", []):
            if "blob" in e:
                yield f"ub{e['size']}", f"s{r['s']}b{r['b']}", fr.blobs.get(e["blob"], b"")
            elif "sblob" in e:
                yield f"sb{e['size']}", f"s{r['s']}b{r['b']}", fr.blobs.get(e["sblob"], b"")
    if d.get("pc"):
        yield f"pc{len(d['pc']) // 2}", "pc", bytes.fromhex(d["pc"])


def u32s(b):
    return struct.unpack(f"<{len(b) // 4}I", b[:len(b) // 4 * 4])


def candidates(fr):
    """Fields (source, offset, half) whose values are always filled bindless slots or sentinels, with the
    images they name: the raw material for FIELDS. Small values (<= 8) alone don't count."""
    index, seen = slots(fr), defaultdict(Counter)
    for d in fr.draws() + [x for k, x in fr.ops if k == "dispatch"]:
        for src, _, b in buffers(fr, d):
            for k, w in enumerate(u32s(b[:STORAGE_SCAN])):
                for half, v in (("", w), (".lo", w & 0xFFFF), (".hi", w >> 16)):
                    seen[(src, k * 4, half)][v] += 1
    out = []
    for key, values in sorted(seen.items()):
        hits = {v: n for v, n in values.items() if v in index}
        if hits and all(v in index or v in SENTINELS for v in values) and any(v > 8 for v in hits):
            out.append((key, {v: label(index[v]) for v in hits}))
    return out


def ident(i):
    return f"{i['fmt']} {i['w']}x{i['h']} m{i['m']}" + (f" l{i['l']}" if i.get("l", 1) > 1 else "")


def label(i):
    return ident(i) + (f" {i['texture']}" if i.get("texture") else f" #{i['hash'][-8:]}" if i.get("hash") else "")


def matches(src, bind, size, field_src, stride):
    if field_src in (src, bind):
        return True
    m = re.fullmatch(r"sb(\d+)x(\d+)", field_src)
    return bool(m) and src.startswith("sb") and size // stride == int(m.group(1))


def used(fr, d):
    """[(how bound, image)]: the draw's directly bound images ("s<set>b<binding>"), then the bindless slots
    FIELDS select ("[<index>] <what>")."""
    out = [(f"s{r['s']}b{r['b']}", e["view"]["img"]) for r in d.get("res") or [] for e in r.get("el", [])
           if (e.get("view") or {}).get("img")]
    index = slots(fr)
    for src, bind, b in buffers(fr, d):
        for engine, f_src, stride, off, half, what in FIELDS:
            if engine != getattr(fr, "engine", None) or not matches(src, bind, len(b), f_src, stride or 1):
                continue
            for at in range(off, len(b) - 3, stride) if stride else [off]:
                w = struct.unpack_from("<I", b, at)[0] if at + 4 <= len(b) else 0
                v = w & 0xFFFF if half == ".lo" else w >> 16 if half == ".hi" else w
                if v not in SENTINELS and v in index:
                    out.append((f"[{v}] {what}", index[v]))
    return out


def key(i):
    """Identity across engines: the texture's name when known, else format, size and mips."""
    return i.get("texture") or ident(i)


def compare(ours, real):
    """(group key, message) per difference between two draws' texture sets. A real texture whose content
    several core files share matches ours' name if any of them is it."""
    names = {i["texture"] for _, i in ours if i.get("texture")}
    rkey = lambda i: next((n for n in i.get("alts", []) if n in names), key(i))
    ko, kr = Counter(key(i) for _, i in ours), Counter(rkey(i) for _, i in real)
    out = []
    if ko != kr:
        a, b = sorted((ko - kr).elements()), sorted((kr - ko).elements())
        out.append((("set", str(a), str(b)), f"textures only on ours {a}, only on real {b}"))
    io, ir = {key(i): i for _, i in ours}, {rkey(i): i for _, i in real}
    for k in sorted(set(io) & set(ir)):
        a, b = io[k], ir[k]
        if a.get("texture") and ident(a) != ident(b):
            out.append((("format", k, ident(a), ident(b)), f"{k}: ours {ident(a)}, real {ident(b)}"))
        elif not a.get("texture") and a.get("hash") and b.get("hash") and a["hash"] != b["hash"]:
            out.append((("content", k), f"{k}: content differs (#{a['hash'][-8:]} vs #{b['hash'][-8:]})"))
    return out


def name(fr, core=None):
    """Sets each image's `texture`: its resource-path label, else the core texture its content matches
    (`alts`: every core file with that content)."""
    unnamed = []
    for i in images(fr):
        if TEXTURE_NAME.search(i.get("name", "")):
            i["texture"] = gpu_vtex.source_name(i["name"])
        elif i.get("hash") and not i.get("hmip"):
            unnamed.append(i)
    wanted = {(i["w"], i["h"], i.get("l", 1), i["m"]) for i in unnamed}
    found = gpu_vtex.index(core, wanted) if core and wanted else {}
    for i in unnamed:
        if i["hash"] in found:
            i["alts"] = found[i["hash"]]
            i["texture"] = i["alts"][0] if len(i["alts"]) == 1 else f"{i['alts'][0]} (+{len(i['alts']) - 1} same)"
