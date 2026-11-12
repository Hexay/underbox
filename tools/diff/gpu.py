#!/usr/bin/env python3
"""GPU frame captures from tools/diff/gpu/ (docs/differential-gpu.md).

usage: gpu.py list <frame.jsonl>                  normalized per-pass/per-draw listing
       gpu.py diff <ours.jsonl> <real.jsonl>      where the two frames diverge (gpu_diff.py)
       gpu.py png <capture-dir> <out-dir>         render-target dumps as PNG
"""
import json
import math
import os
import struct
import sys
from collections import Counter

sys.path[:0] = [os.path.dirname(os.path.abspath(__file__)), os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")]


class Frame:
    def __init__(self, path):
        self.pipes, self.blobs, self.bindless = {}, {}, {}
        self.passes, self.ops, self.dumps = [], [], []
        self.end = {}
        cur = None
        for line in open(path):
            r = json.loads(line)
            t = r["t"]
            if t == "pipe":
                self.pipes[r["id"]] = r
            elif t == "blob":
                self.blobs[r["h"]] = bytes.fromhex(r["hex"])
            elif t == "bindless":
                self.bindless[r["key"]] = r
            elif t == "pass":
                cur = dict(r, draws=[], clears=0)
                self.passes.append(cur)
                self.ops.append(("pass", cur))
            elif t == "end":
                cur = None
            elif t == "draw":
                r["pipeinfo"] = self.pipes.get(r.get("pipe"), {})
                (cur["draws"] if cur else self.ops).append(r if cur else ("draw", r))
            elif t == "clear" and cur:
                cur["clears"] += 1
            elif t in ("dispatch", "xfer"):
                if t == "dispatch":
                    r["pipeinfo"] = self.pipes.get(r.get("pipe"), {})
                self.ops.append((t, r))
            elif t == "dump":
                self.dumps.append(r)
            elif t == "present":
                self.end = r

    def draws(self):
        return [d for p in self.passes for d in p["draws"]]


def img(i):
    """An image's identity without handles: format, size, mips, layers (and its debug name, if any)."""
    if not i:
        return "?"
    s = f"{i['fmt']} {i['w']}x{i['h']}" + (f"x{i['d']}" if i.get("d", 1) > 1 else "") + f" m{i['m']}"
    return s + (f" l{i['l']}" if i.get("l", 1) > 1 else "") + (f" s{i['s']}" if i.get("s", 1) > 1 else "")


def att(a):
    v = a.get("view") or {}
    s = img(v.get("img")) if v.get("img") else a["fmt"]
    op = a["load"] + (f"({','.join(fmt_num(x) for x in a['clear'])})" if "clear" in a else "")
    return f"{s} {op}/{a['store']}"


def fmt_num(x):
    return f"{x:.4g}" if isinstance(x, float) else str(x)


def mesh(d):
    """Structural mesh key: per sub-draw index count, vertex span and relative-index hash (or vertex count)."""
    return tuple((s["n"], s.get("vspan", 0), s.get("ih", "")[:8], s["inst"]) for s in d.get("sub", []))


def shaders(p):
    return " ".join(f"{s['stage'][:4].lower()}:{s['hash'][:8]}" + (f"({s['entry']})" if s["entry"] not in ("main", "") else "")
                    for s in p.get("stages", []))


def state_line(st):
    if not st:
        return "no state"
    bl = ",".join("off" if not b["en"] else f"{b['c']}|{b['a']}" for b in st.get("blend", [])) or "-"
    masks = ",".join(b["mask"] or "none" for b in st.get("blend", [])) or "-"
    depth = f"depth {st['dcmp']}{' w' if st['dwrite'] else ''}" if st["dtest"] else ("depth off w" if st["dwrite"] else "depth off")
    return f"{st['topo']} {st['poly']} cull {st['cull']} {st['front']} {depth}{' bias' + str(st['bias']) if 'bias' in st else ''} blend {bl} mask {masks}"


def resources(fr, res):
    """Per-draw resources: textures and samplers by identity, uniform blobs by size and hash, bindless arrays by fill."""
    tex, ubs, other = [], [], []
    for r in res or []:
        if "bindless" in r:
            b = fr.bindless.get(r["bindless"], {})
            other.append(f"{r['type']}[{b.get('filled', '?')}/{b.get('count', '?')}]")
            continue
        for e in r.get("el", []):
            if "view" in e:
                tex.append(img((e["view"] or {}).get("img")) + (f" {smp(e['smp'])}" if e.get("smp") else ""))
            elif "smp" in e:
                tex.append(smp(e["smp"]))
            elif "blob" in e:
                ubs.append((e.get("size", 0), e["blob"]))
            elif "size" in e:
                other.append(f"{r['type'][:7]}:{e['size']}B")
    return tex, ubs, other


def smp(s):
    if not s:
        return "smp?"
    addr = "/".join(a[:5] for a in s["addr"])
    return f"[{s['min']}{s['mag']}{s['mip']} {addr}{' a' + fmt_num(s['aniso']) if s['aniso'] else ''}{' cmp ' + s['cmp'] if 'cmp' in s else ''}]"


def listing(fr):
    out = [f"frame: {len(fr.passes)} passes, {len(fr.draws())} draws, "
           f"{sum(1 for k, _ in fr.ops if k == 'dispatch')} dispatches, {sum(1 for k, _ in fr.ops if k == 'xfer')} transfers, "
           f"{fr.end.get('unrecorded', 0)} unrecorded command buffers"]
    arrays = Counter((b["type"], b["filled"], b["count"], tuple(sorted(b["imgs"]))) for b in fr.bindless.values())
    for (t, filled, count, imgs), n in sorted(arrays.items()):
        out.append(f"bindless {t} {filled}/{count} filled, in {n} sets" + (f": {'; '.join(imgs[:12])}" if imgs else ""))
    for kind, x in fr.ops:
        if kind == "pass":
            area = x["area"]
            out.append(f"P{x['i']} sel={x['sel']} {area[2]}x{area[3]}{' [' + x['label'] + ']' if x.get('label') else ''}"
                       f" {len(x['draws'])} draws{', ' + str(x['clears']) + ' clears' if x['clears'] else ''}{' DUMPED' if x.get('dumped') else ''}")
            for a in x["att"]:
                out.append(f"    {'depth' if a['depth'] else 'color'} {att(a)}")
            for d in x["draws"]:
                out.extend(draw_lines(fr, d))
        elif kind == "dispatch":
            out.append(f"C {x['cmd']} {tuple(x['group'])} pipe{x['pipe']} {shaders(x['pipeinfo'])}{' [' + x['label'] + ']' if x.get('label') else ''}")
        elif kind == "xfer":
            dst = x["dst"] or {}
            out.append(f"X {x['op']} -> {img(dst) if 'fmt' in dst else str(dst.get('buffer', '?')) + 'B buffer'}")
        elif kind == "draw":
            out.extend(draw_lines(fr, x))
    return "\n".join(out)


def draw_lines(fr, d):
    subs = " ".join(f"n={n}" + (f" vspan={v} ih={h}" if h else "") + (f" x{i}" if i != 1 else "") for n, v, h, i in mesh(d))
    tex, ubs, other = resources(fr, d.get("res"))
    lines = [f"  D{d['i']} {d['cmd']} {subs}{' SKIPPED' if d.get('skipped') else ''}{' [' + d['label'] + ']' if d.get('label') else ''}",
             f"      pipe{d['pipe']} {shaders(d['pipeinfo'])} va={','.join(a[2] for a in d['pipeinfo'].get('va', []))}",
             f"      {state_line(d.get('state'))}"]
    if tex:
        lines.append("      tex " + "; ".join(tex))
    if ubs or other or d.get("pc"):
        lines.append("      " + " ".join([f"ub {n}B:{h[:8]}" for n, h in ubs] + other + ([f"pc {len(d['pc']) // 2}B"] if d.get("pc") else [])))
    return lines


def half(h):
    return struct.unpack("<e", struct.pack("<H", h))[0]


def uf(bits, mant):
    e, m = bits >> mant, bits & ((1 << mant) - 1)
    return 0.0 if e == 0 and m == 0 else (m / (1 << mant)) * 2.0 ** -14 if e == 0 else (1 + m / (1 << mant)) * 2.0 ** (e - 15)


def to_rgb(fmt, w, h, raw):
    """Texels to 8-bit RGB for viewing: float formats clamped and sRGB-encoded, depth stretched over its range."""
    n = w * h
    enc = lambda x: int(255 * (12.92 * x if x <= 0.0031308 else 1.055 * x ** (1 / 2.4) - 0.055) + 0.5) if x > 0 else 0
    if fmt.startswith(("R8G8B8A8", "B8G8R8A8")):
        px = [(raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2]) for i in range(n)]
        return bytes(c for p in px for c in (p[::-1] if fmt.startswith("B") else p))
    if fmt == "R16G16B16A16Sfloat":
        v = struct.unpack(f"<{n * 4}H", raw[:n * 8])
        return bytes(enc(min(1.0, half(v[i * 4 + c]))) for i in range(n) for c in range(3))
    if fmt == "B10G11R11UfloatPack32":
        v = struct.unpack(f"<{n}I", raw[:n * 4])
        return bytes(enc(min(1.0, uf(x >> s & m, k))) for x in v for s, m, k in ((0, 2047, 6), (11, 2047, 6), (22, 1023, 5)))
    if fmt.startswith("A2B10G10R10"):
        v = struct.unpack(f"<{n}I", raw[:n * 4])
        return bytes((x >> s & 1023) >> 2 for x in v for s in (0, 10, 20))
    if fmt in ("D32Sfloat", "R32Sfloat", "D32SfloatS8Uint"):
        v = list(struct.unpack(f"<{n}f", raw[:n * 4]))
    elif fmt in ("X8D24UnormPack32", "D24UnormS8Uint"):
        v = [x & 0xFFFFFF for x in struct.unpack(f"<{n}I", raw[:n * 4])]
    elif fmt == "R8Unorm":
        v = list(raw[:n])
    else:
        return None
    fin = [x for x in v if math.isfinite(x)]
    lo, hi = (min(fin), max(fin)) if fin else (0, 1)
    g = bytes(int(255 * (x - lo) / (hi - lo)) if hi > lo and math.isfinite(x) else 0 for x in v)
    return bytes(c for x in g for c in (x, x, x))


def png(cap, out):
    from ppm2png import write_png
    jl = os.path.join(cap, "frame.jsonl")
    if not os.path.exists(jl):
        return
    for d in Frame(jl).dumps:
        rgb = to_rgb(d["fmt"], d["w"], d["h"], open(os.path.join(cap, d["file"]), "rb").read())
        if rgb is None:
            print(f"png: {d['file']}: no conversion for {d['fmt']}")
            continue
        os.makedirs(out, exist_ok=True)
        dst = os.path.join(out, d["file"].replace(".raw", ".png"))
        write_png(dst, d["w"], d["h"], rgb)
        print(f"dump P{d['pass']} att {d['att']} draw {d['draw']} {d['fmt']} {d['w']}x{d['h']}: {dst}")


def main(argv):
    if len(argv) == 3 and argv[1] == "list":
        print(listing(Frame(argv[2])))
    elif len(argv) == 4 and argv[1] == "diff":
        import gpu_diff
        print(gpu_diff.report(Frame(argv[2]), Frame(argv[3])))
    elif len(argv) == 4 and argv[1] == "png":
        png(argv[2], argv[3])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv)
