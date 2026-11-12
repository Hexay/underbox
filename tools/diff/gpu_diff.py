"""Where two GPU frame captures diverge (docs/differential-gpu.md), for gpu.py diff.

Passes are aligned by role (colour attachment count, depth or not, render area), draws within a pass by
mesh (index count, vertex span, hash of indices relative to their minimum), then by mesh in any order.
Handles never take part. Matched draws compare effective state, vertex formats, textures and samplers by
identity, and uniform contents paired by size. Divergences are grouped by kind and value, ranked by kind,
and the first in frame order is called out.
"""
import struct
from collections import Counter, defaultdict
from difflib import SequenceMatcher

import gpu_tex
from gpu import att, mesh, resources

SEV = {"pass": 0, "attachment": 1, "draw count": 1, "draw": 2, "state": 2, "vertex format": 3, "textures": 3,
       "bindless": 4, "uniform": 4, "dispatch": 4, "push constants": 5, "draw order": 5, "transfers": 6}
STATE = ("topo", "poly", "cull", "front", "dtest", "dwrite", "dcmp", "bias", "stencil", "samples", "a2c", "clamp")


class Divs:
    def __init__(self):
        self.groups = defaultdict(list)

    def add(self, kind, key, where, msg):
        self.groups[(kind, key)].append((where, msg))

    def first(self):
        best = min(((w, k, m) for (k, _), v in self.groups.items() for w, m in v), default=None)
        return best and f"{where(best[0])} (ours' numbering): {best[1]}: {best[2]}"

    def ranked(self):
        rows = [(SEV[k[0]], -len(v), k, v) for k, v in self.groups.items()]
        for sev, n, (kind, _), v in sorted(rows, key=lambda r: (r[0], r[1], min(w for w, _ in r[3]))):
            yield kind, -n, min(v)


def where(w):
    """A place in ours' frame; real-only passes sit after the matched pass before them (D999)."""
    p, d = w
    if d == 999:
        return f"after P{p}" if p >= 0 else "start"
    return f"P{p}" + (f" D{d}" if d >= 0 else "")


def role(p):
    return (sum(1 for a in p["att"] if not a["depth"]), any(a["depth"] for a in p["att"]), p["area"][2], p["area"][3])


def role_text(p):
    c, d, w, h = role(p)
    return f"{w}x{h} {c} colour{'+depth' if d else ''}{' [' + p['label'] + ']' if p.get('label') else ''}"


def align(a, b, key, loose=None):
    """Index pairs (i, j) of a and b: in-order matches by key; inside each differing run, in-order matches
    by the looser key; same-length leftovers side by side."""
    ka, kb = [key(x) for x in a], [key(x) for x in b]
    pairs, only_a, only_b = [], [], []
    for op, i1, i2, j1, j2 in SequenceMatcher(None, ka, kb, autojunk=False).get_opcodes():
        if op == "equal":
            pairs += list(zip(range(i1, i2), range(j1, j2)))
        elif loose:
            sub, oa, ob = align(a[i1:i2], b[j1:j2], loose)
            pairs += [(i1 + i, j1 + j) for i, j in sub]
            only_a += [i1 + i for i in oa]
            only_b += [j1 + j for j in ob]
        elif op == "replace" and i2 - i1 == j2 - j1:
            pairs += list(zip(range(i1, i2), range(j1, j2)))
        else:
            only_a += range(i1, i2)
            only_b += range(j1, j2)
    return sorted(pairs), only_a, only_b


def shape(d):
    """A draw's meshes without instance counts: engines batch instances differently."""
    return tuple(m[:3] for m in mesh(d))


def content(p):
    return role(p) + (tuple(sorted({shape(d) for d in p["draws"]})),)


def floats(b):
    return struct.unpack(f"<{len(b) // 4}f", b[:len(b) // 4 * 4])


def uniforms(ours, real, uo, ur, w, divs):
    uo, left = sorted(set(uo)), sorted(set(ur))
    for size, h in uo:
        same = next((x for x in left if x[1] == h), None) or next((x for x in left if x[0] == size), None)
        if not same:
            divs.add("uniform", ("size", size), w, f"a {size}-byte uniform buffer only on ours")
            continue
        left.remove(same)
        if same[1] == h:
            continue
        a, b = ours.blobs.get(h, b""), real.blobs.get(same[1], b"")
        diff = [i for i, (x, y) in enumerate(zip(floats(a), floats(b)))
                if a[i * 4:i * 4 + 4] != b[i * 4:i * 4 + 4] and not abs(x - y) <= 1e-4 * max(1.0, abs(x), abs(y))]
        if diff:
            i = diff[0]
            divs.add("uniform", (size, i * 4), w, f"{size}-byte uniform: {len(diff)}/{len(a) // 4} floats differ, first at byte {i * 4}: "
                                                   f"ours {floats(a)[i]:.6g} real {floats(b)[i]:.6g}")
    for size, _ in left:
        divs.add("uniform", ("size", -size), w, f"a {size}-byte uniform buffer only on real")


def compare_draw(ours, real, do, dr, w, divs):
    so, sr = do.get("state", {}), dr.get("state", {})
    for f in STATE:
        if so.get(f) != sr.get(f):
            divs.add("state", (f, str(so.get(f)), str(sr.get(f))), w, f"{f} ours {so.get(f)} real {sr.get(f)}")
    bo, br = so.get("blend", []), sr.get("blend", [])
    for i in range(max(len(bo), len(br))):
        x, y = (bo[i] if i < len(bo) else None), (br[i] if i < len(br) else None)
        if x != y:
            divs.add("state", (f"blend{i}", str(x), str(y)), w, f"blend attachment {i} ours {x} real {y}")
    vo = [a[2] for a in do["pipeinfo"].get("va", [])]
    vr = [a[2] for a in dr["pipeinfo"].get("va", [])]
    if sorted(vo) != sorted(vr):
        divs.add("vertex format", (str(vo), str(vr)), w, f"vertex attributes ours {vo} real {vr}")
    to, uo, _ = resources(ours, do.get("res"))
    tr, ur, _ = resources(real, dr.get("res"))
    for k, msg in gpu_tex.compare(gpu_tex.used(ours, do), gpu_tex.used(real, dr)):
        divs.add("textures", k, w, msg)
    so_, sr_ = Counter(t.split(" [", 1)[1] for t in to if " [" in t), Counter(t.split(" [", 1)[1] for t in tr if " [" in t)
    if so_ != sr_:
        a, b = sorted((so_ - sr_).elements()), sorted((sr_ - so_).elements())
        divs.add("textures", ("samplers", str(a), str(b)), w, f"samplers only on ours [{'; ['.join(a)}, only on real [{'; ['.join(b)}")
    uniforms(ours, real, uo, ur, w, divs)
    lo, lr = len(do.get("pc", "")) // 2, len(dr.get("pc", "")) // 2
    if lo != lr:
        divs.add("push constants", (lo, lr), w, f"push constants ours {lo} bytes, real {lr}")


def compare_pass(ours, real, po, pr, divs):
    w = (po["i"], -1)
    ao, ar = po["att"], pr["att"]
    for i in range(max(len(ao), len(ar))):
        x, y = (att(ao[i]) if i < len(ao) else "-"), (att(ar[i]) if i < len(ar) else "-")
        if x != y:
            divs.add("attachment", (role(pr), i, x, y), w, f"attachment {i} ours {x} real {y}")
    do, dr = po["draws"], pr["draws"]
    # Per mesh: instances drawn and in how many draws; the first draw of each mesh pairs for state.
    io, ir, no, nr = Counter(), Counter(), Counter(), Counter()
    for ds, inst, n in ((do, io, no), (dr, ir, nr)):
        for d in ds:
            for m in mesh(d):
                inst[m[:3]] += m[3]
                n[m[:3]] += 1
    if io != ir:
        divs.add("draw count", (role(pr), len(do), len(dr)), w, f"{role_text(pr)}: ours {len(do)} draws, real {len(dr)}")
    for m in sorted(set(no) | set(nr), key=str):
        name = f"mesh n={m[0]} vspan={m[1]}" + (f" ih={m[2]}" if m[2] else "")
        if not nr[m] or not no[m]:
            side, n = ("ours", io[m]) if no[m] else ("real", ir[m])
            divs.add("draw", (side, m), (po["i"], next((i for i, d in enumerate(do) if m in shape(d)), len(do))),
                     f"{name} only on {side} ({n} instances); {role_text(pr)}")
        elif io[m] != ir[m]:
            divs.add("draw", ("instances", m), (po["i"], -1), f"{name}: ours {io[m]} instances, real {ir[m]}")
        elif no[m] != nr[m]:
            divs.add("draw order", ("batching", no[m], nr[m]), (po["i"], -1),
                     f"{name}: {io[m]} instances in {no[m]} draws on ours, {nr[m]} on real")
    first = lambda ds: {s: i for i, d in reversed(list(enumerate(ds))) for s in [shape(d)]}
    fo, fr_ = first(do), first(dr)
    for s in sorted(set(fo) & set(fr_), key=lambda s: fo[s]):
        compare_draw(ours, real, do[fo[s]], dr[fr_[s]], (po["i"], fo[s]), divs)


def summary(fr):
    k = Counter(kind for kind, _ in fr.ops)
    return f"{len(fr.passes)} passes, {len(fr.draws())} draws, {k['dispatch']} dispatches, {k['xfer']} transfers"


def report(ours, real):
    divs = Divs()
    out = ["GPU frame: ours vs real", f"  ours: {summary(ours)}", f"  real: {summary(real)}"]
    pairs, only_o, only_r = align(ours.passes, real.passes, content, role)
    for i in only_o:
        divs.add("pass", ("ours", role(ours.passes[i])), (i, -1), f"only on ours: {role_text(ours.passes[i])}, {len(ours.passes[i]['draws'])} draws")
    for j in only_r:
        near = max((i for i, jj in pairs if jj < j), default=-1)
        divs.add("pass", ("real", role(real.passes[j])), (near, 999), f"only on real (its P{j}): {role_text(real.passes[j])}, {len(real.passes[j]['draws'])} draws")
    for i, j in pairs:
        compare_pass(ours, real, ours.passes[i], real.passes[j], divs)
    gd = lambda fr: Counter(tuple(d["group"]) for k, d in fr.ops if k == "dispatch")
    if gd(ours) != gd(real):
        divs.add("dispatch", "groups", (0, -1), f"dispatch group sizes only on ours {sorted((gd(ours) - gd(real)).elements())}, "
                                                f"only on real {sorted((gd(real) - gd(ours)).elements())}")
    bo = Counter(i for b in ours.bindless.values() for i in b["imgs"])
    br = Counter(i for b in real.bindless.values() for i in b["imgs"])
    if bo != br:
        a, b = (bo - br), (br - bo)
        divs.add("bindless", "imgs", (0, -1), f"bindless textures: {sum(bo.values())} ours, {sum(br.values())} real; "
                                              f"only ours {sorted(a.items())[:8]}, only real {sorted(b.items())[:8]}")
    out.append(f"\nFirst divergence: {divs.first() or 'none'}")
    out.append("\nDivergences by kind (count, first place, detail):")
    for kind, n, (w, msg) in divs.ranked():
        out.append(f"  {kind:14} x{n:<4} {where(w):9} {msg}")
    out.append("\nPass alignment (ours <-> real):")
    for i, j in pairs:
        po, pr = ours.passes[i], real.passes[j]
        out.append(f"  P{i:<3} <-> P{j:<3} {role_text(pr)}: draws {len(po['draws'])}/{len(pr['draws'])}")
    for i in only_o:
        out.append(f"  P{i:<3}  -  ours only: {role_text(ours.passes[i])}")
    for j in only_r:
        out.append(f"   -  P{j:<3} real only: {role_text(real.passes[j])}")
    return "\n".join(out)
