#!/usr/bin/env python3
"""Compare two call-tap runs (docs/differential-tap.md): print the first stable divergences.

usage: tap.py <ours-dir> <real-dir> [--calibrate <real2-dir>] [-n 20] [--context 6] [--digits 4]
              [--slots GLOB] [--from-tick T] [--keep-ptrs] [--kind return] [--unique]

Calls are grouped per (tick, slot) and compared as multisets of (args, return) after normalising:
pointer ordinals masked (unless --keep-ptrs), floats rounded to --digits significant digits, values
under 1e-4 printed as 0. With --calibrate, a (tick, slot) group that differs between the two real
runs is noise and is skipped. Each divergence is classified:
  return   the same arguments got a different return value (or out-arguments)
  args     the slot was called with different arguments (usually a consequence of an earlier one)
  extra    ours made calls real didn't
  missing  real made calls ours didn't
"""
import argparse
import fnmatch
import glob
import os
import re
import sys
from collections import Counter, defaultdict

PTR = re.compile(r"\bp\d+\b")
FLOAT = re.compile(r"-?\d+\.\d+(?:e[-+]?\d+)?|-?\d+e[-+]?\d+|-?inf|NaN")


class Rec:
    __slots__ = ("seq", "tick", "depth", "side", "name", "args", "ret", "thread", "key")

    def __init__(self, f, thread):
        self.seq, self.tick, self.depth = int(f[0]), int(f[1]), int(f[2])
        self.side, self.name, self.args, self.ret = f[3], f[4], f[5], f[6]
        self.thread = thread
        self.key = None

    def text(self):
        return f"{self.name}({self.args}) = {self.ret}"


def load(d):
    recs = []
    for path in sorted(glob.glob(os.path.join(d, "t*.tap"))):
        thread = os.path.basename(path)[:-4]
        with open(path, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                if line.startswith("#"):
                    continue
                f = line.rstrip("\n").split("\t")
                if len(f) == 7:
                    recs.append(Rec(f, thread))
    recs.sort(key=lambda r: r.seq)
    return recs


def normaliser(digits, keep_ptrs):
    def num(m):
        s = m.group(0)
        try:
            x = float(s)
        except ValueError:
            return s
        if x != x or abs(x) == float("inf"):
            return s
        return "0" if abs(x) < 1e-4 else f"{x:.{digits}g}"

    def norm(s):
        if not keep_ptrs:
            s = PTR.sub("p", s)
        return FLOAT.sub(num, s)

    return norm


def group(recs, norm, slots, from_tick):
    g = defaultdict(list)
    for r in recs:
        if r.tick < from_tick or (slots and not fnmatch.fnmatchcase(r.name, slots)):
            continue
        r.key = (norm(r.args), norm(r.ret))
        g[(r.tick, r.name)].append(r)
    return g


def differs(a, b):
    return Counter(r.key for r in a) != Counter(r.key for r in b)


def classify(ours, real):
    """Pair the unmatched calls of one (tick, slot) group: [(kind, ours rec|None, real rec|None)]."""
    left = Counter(r.key for r in real)
    extra = []
    for r in ours:
        if left[r.key] > 0:
            left[r.key] -= 1
        else:
            extra.append(r)
    right = Counter(r.key for r in ours)
    missing = []
    for r in real:
        if right[r.key] > 0:
            right[r.key] -= 1
        else:
            missing.append(r)
    def ins(r):
        return r.key[0].split(" | ")[0]

    out = []
    for o in extra:
        m, kind = next((m for m in missing if ins(m) == ins(o)), None), "return"
        if m is None and missing:
            m, kind = missing[0], "args"
        if m is None:
            out.append(("extra", o, None))
            continue
        missing.remove(m)
        out.append((kind, o, m))
    out += [("missing", None, m) for m in missing]
    return out


def tick_positions(recs):
    """Each record's position within its tick, 0..1, so the two engines' orders compare."""
    first, count = {}, Counter(r.tick for r in recs)
    pos = {}
    for r in recs:
        i = first.setdefault(r.tick, 0)
        pos[id(r)] = i / max(count[r.tick], 1)
        first[r.tick] = i + 1
    return pos


def context(recs_by_thread, index, rec, k):
    if rec is None:
        return []
    lst = recs_by_thread[rec.thread]
    i = index[id(rec)]
    return lst[max(0, i - k) : i]


def threads(recs):
    by, index = defaultdict(list), {}
    for r in recs:
        index[id(r)] = len(by[r.thread])
        by[r.thread].append(r)
    return by, index


def summary(label, recs):
    ticks = [r.tick for r in recs] or [0]
    return f"{label}: {len(recs)} calls, {len({r.thread for r in recs})} threads, ticks {min(ticks)}-{max(ticks)}"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("ours")
    ap.add_argument("real")
    ap.add_argument("--calibrate")
    ap.add_argument("-n", type=int, default=20)
    ap.add_argument("--context", type=int, default=6)
    ap.add_argument("--digits", type=int, default=4)
    ap.add_argument("--slots")
    ap.add_argument("--from-tick", type=int, default=0)
    ap.add_argument("--keep-ptrs", action="store_true")
    ap.add_argument("--kind", help="show only this kind (return, args, extra, missing)")
    ap.add_argument("--unique", action="store_true", help="show only each slot's first divergence")
    a = ap.parse_args()
    norm = normaliser(a.digits, a.keep_ptrs)

    ours, real = load(a.ours), load(a.real)
    if not ours or not real:
        sys.exit(f"tap: no records in {a.ours if not ours else a.real}")
    print(summary("ours", ours))
    print(summary("real", real))
    go, gr = group(ours, norm, a.slots, a.from_tick), group(real, norm, a.slots, a.from_tick)
    unstable = set()
    if a.calibrate:
        cal = load(a.calibrate)
        gc = group(cal, norm, a.slots, a.from_tick)
        unstable = {k for k in set(gr) | set(gc) if differs(gr.get(k, []), gc.get(k, []))}
        print(summary("real (calibration)", cal) + f"; {len(unstable)} of {len(set(gr) | set(gc))} (tick, slot) groups differ between real runs and are skipped")

    keys = set(go) | set(gr)
    diverged = [k for k in keys if k not in unstable and differs(go.get(k, []), gr.get(k, []))]
    print(f"compared {len(keys)} (tick, slot) groups: {len(diverged)} diverge{'' if a.calibrate else ' (uncalibrated)'}")
    po, pr = tick_positions(ours), tick_positions(real)
    found = []
    for k in diverged:
        for kind, o, r in classify(go.get(k, []), gr.get(k, [])):
            at = po[id(o)] if o is not None else pr[id(r)]
            found.append((k[0], at, kind, o, r))
    found.sort(key=lambda f: (f[0], f[1]))

    by_slot = Counter(f[3].name if f[3] else f[4].name for f in found)
    kinds = Counter(f[2] for f in found)
    print("kinds: " + ", ".join(f"{k} {n}" for k, n in kinds.most_common()))
    print("slots with most divergent calls: " + ", ".join(f"{s} {n}" for s, n in by_slot.most_common(12)))
    first_ret = next((f for f in found if f[2] == "return"), None)
    if first_ret:
        print(f"first return divergence: tick {first_ret[0]} {first_ret[3].name}")

    shown, seen = [], set()
    for f in found:
        name = (f[3] or f[4]).name
        if (a.kind and f[2] != a.kind) or (a.unique and name in seen):
            continue
        seen.add(name)
        shown.append(f)
    to, io = threads(ours)
    tr, ir = threads(real)
    for n, (tick, _, kind, o, r) in enumerate(shown[: a.n], 1):
        rec = o or r
        print(f"\n{n}. tick {tick} {kind}: {rec.side} {rec.name}")
        if o is not None:
            print(f"   ours: ({o.args}) = {o.ret}   [seq {o.seq}, {o.thread}]")
        if r is not None:
            print(f"   real: ({r.args}) = {r.ret}   [seq {r.seq}, {r.thread}]")
        for label, recs, idx, x in (("ours", to, io, o), ("real", tr, ir, r)):
            ctx = context(recs, idx, x, a.context)
            if ctx:
                print(f"   before, {label}:")
                for c in ctx:
                    print(f"     {'  ' * min(c.depth, 4)}{c.side} {c.text()[:160]}")


if __name__ == "__main__":
    main()
