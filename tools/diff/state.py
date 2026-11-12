#!/usr/bin/env python3
"""Rank divergences between two StateDump captures (tools/diff/StateDump.cs, docs/differential-state.md).

usage: state.py <ours.jsonl> <real.jsonl> [--noise real2.jsonl] [--abs 0.01] [--rel 0.001] [--deg 0.1] [--top 60]

Objects match by path, components and bodies by key, properties by dotted name. A property that
differs between real and --noise (a second real run) is ignored. Prints one row per diverging
property: first tick, severity, object, property, ours vs real.
"""
import argparse
import json
import math
import sys
from collections import defaultdict

# Severity classes, worst first.
MISSING_OBJECT, MISSING_COMPONENT, KIND, VALUE, NUMBER = range(5)
CLASS_NAME = {MISSING_OBJECT: "object", MISSING_COMPONENT: "component", KIND: "kind", VALUE: "value", NUMBER: "number"}


def load(path):
    """{(path, key): {tick: value}}; lines that don't parse are counted, not fatal."""
    out = defaultdict(dict)
    bad = 0
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            try:
                rec = json.loads(line)
                out[(rec["p"], rec["k"])][rec["t"]] = rec["v"]
            except (ValueError, KeyError, TypeError):
                bad += 1
    if bad:
        print(f"state: {path}: {bad} unreadable lines", file=sys.stderr)
    return out


def is_leaf(v):
    if not isinstance(v, (dict, list)):
        return True
    if isinstance(v, list):
        return all(isinstance(x, (int, float)) and not isinstance(x, bool) for x in v)
    return len(v) == 1 and next(iter(v)) in ("q", "ang", "$err", "count")


def flatten(v, prefix=""):
    if is_leaf(v):
        yield prefix, v
        return
    items = v.items() if isinstance(v, dict) else enumerate(v)
    for k, x in items:
        yield from flatten(x, f"{prefix}.{k}" if prefix else str(k))


def quat_angle(a, b):
    dot = abs(sum(x * y for x, y in zip(a, b)))
    na = math.sqrt(sum(x * x for x in a)) * math.sqrt(sum(y * y for y in b))
    return math.degrees(2 * math.acos(min(1.0, dot / na))) if na else 0.0


def wrap(d):
    return abs((d + 180) % 360 - 180)


class Tol:
    def __init__(self, a):
        self.abs, self.rel, self.deg = a.abs, a.rel, a.deg

    def compare(self, a, b):
        """None when equal within tolerance, else (class, score): score is the excess over tolerance."""
        if a == b:
            return None
        num = (int, float)
        if isinstance(a, bool) or isinstance(b, bool) or a is None or b is None:
            return (KIND if type(a) is not type(b) else VALUE, 1.0)
        if isinstance(a, num) and isinstance(b, num):
            return self.scalar(a, b)
        if isinstance(a, list) and isinstance(b, list):
            if len(a) != len(b):
                return (KIND, 1.0)
            worst = max((self.scalar(x, y) or (NUMBER, 0.0))[1] for x, y in zip(a, b))
            return (NUMBER, worst) if worst > 0 else None
        if isinstance(a, dict) and isinstance(b, dict) and a.keys() == b.keys():
            k = next(iter(a))
            # Non-finite floats arrive as strings ("NaN").
            if k in ("q", "ang") and not all(isinstance(x, (int, float)) for x in a[k] + b[k]):
                return (VALUE, 1.0)
            if k == "q":
                d = quat_angle(a["q"], b["q"])
                return (NUMBER, d / self.deg) if d > self.deg else None
            if k == "ang":
                d = max(wrap(x - y) for x, y in zip(a["ang"], b["ang"]))
                return (NUMBER, d / self.deg) if d > self.deg else None
            if k == "count":
                return (VALUE, 1.0)
        if isinstance(a, str) and isinstance(b, str):
            return (VALUE, 1.0)
        return (KIND, 1.0)

    def scalar(self, a, b):
        if not all(isinstance(x, (int, float)) for x in (a, b)):
            return None if a == b else (KIND, 1.0)
        d = abs(a - b)
        lim = self.abs + self.rel * max(abs(a), abs(b))
        return (NUMBER, d / lim) if d > lim else None


def divergences(ours, real, tol):
    """{(path, key, prop): (first_tick, class, worst_score, ticks, ours_value, real_value)}."""
    found = {}

    def note(where, tick, cls, score, a, b):
        cur = found.get(where)
        if cur is None:
            found[where] = [tick, cls, score, 1, a, b]
            return
        cur[3] += 1
        if tick < cur[0]:
            cur[0], cur[4], cur[5] = tick, a, b
        cur[1] = min(cur[1], cls)
        cur[2] = max(cur[2], score)

    objects = {p for p, k in ours if k == "#"} | {p for p, k in real if k == "#"}
    for key in sorted(set(ours) | set(real)):
        path, comp = key
        mine, theirs = ours.get(key, {}), real.get(key, {})
        if not mine or not theirs:
            whole = comp == "#" or (path in objects and not (ours.get((path, "#")) and real.get((path, "#"))))
            if whole and comp != "#":
                continue
            ticks = mine or theirs
            note((path, comp, ""), min(ticks), MISSING_OBJECT if whole else MISSING_COMPONENT, 1.0,
                 "present" if mine else "absent", "present" if theirs else "absent")
            continue
        for tick in sorted(set(mine) & set(theirs)):
            a, b = dict(flatten(mine[tick])), dict(flatten(theirs[tick]))
            for prop in sorted(set(a) | set(b)):
                if prop not in a or prop not in b:
                    note((path, comp, prop), tick, KIND, 1.0, a.get(prop, "<absent>"), b.get(prop, "<absent>"))
                    continue
                d = tol.compare(a[prop], b[prop])
                if d:
                    note((path, comp, prop), tick, d[0], d[1], a[prop], b[prop])
    return found


def num(x):
    return f"{x:.4g}" if isinstance(x, (int, float)) else str(x)


def short(v, width=48):
    if isinstance(v, float):
        s = num(v)
    elif isinstance(v, list) and all(isinstance(x, (int, float)) for x in v):
        s = "(" + ", ".join(map(num, v)) + ")"
    elif isinstance(v, dict) and len(v) == 1 and next(iter(v)) in ("q", "ang"):
        k = next(iter(v))
        s = k + "(" + ", ".join(map(num, v[k])) + ")"
    else:
        s = json.dumps(v, ensure_ascii=False) if not isinstance(v, str) else v
    return s if len(s) <= width else s[: width - 1] + "…"


def generic(path, comp, prop):
    """Component type and property with indices dropped: groups the same divergence across objects."""
    kind = comp.split("[")[0]
    return f"{kind}.{'.'.join(p for p in prop.split('.') if not p.isdigit())}" if prop else kind


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("ours")
    ap.add_argument("real")
    ap.add_argument("--noise", help="a second real capture; what differs between the two real runs is ignored")
    ap.add_argument("--abs", type=float, default=0.01)
    ap.add_argument("--rel", type=float, default=0.001)
    ap.add_argument("--deg", type=float, default=0.1)
    ap.add_argument("--top", type=int, default=60)
    a = ap.parse_args()
    tol = Tol(a)
    ours, real = load(a.ours), load(a.real)
    found = divergences(ours, real, tol)
    noisy = set()
    if a.noise:
        noisy = set(divergences(real, load(a.noise), tol))
        print(f"noise floor: {len(noisy)} properties differ between the two real runs (ignored)")
    rows = [(k, v) for k, v in found.items() if k not in noisy and (k[0], k[1], "") not in noisy]
    ticks = sorted({t for d in (ours, real) for v in d.values() for t in v})
    print(f"ticks {ticks}; objects ours {sum(1 for p, k in ours if k == '#')} real {sum(1 for p, k in real if k == '#')}; "
          f"diverging properties {len(rows)}")
    if not rows:
        print("RESULT: SAME")
        return
    rows.sort(key=lambda r: (r[1][0], r[1][1], -r[1][2], r[0]))
    print(f"\n{'tick':>5} {'class':<9} {'score':>8} {'n':>3}  object :: component.property  ours | real")
    for (path, comp, prop), (tick, cls, score, n, x, y) in rows[: a.top]:
        where = f"{path} :: {comp}" + (f".{prop}" if prop else "")
        print(f"{tick:>5} {CLASS_NAME[cls]:<9} {score:>8.3g} {n:>3}  {where}  {short(x)} | {short(y)}")
    if len(rows) > a.top:
        print(f"... {len(rows) - a.top} more")
    groups = defaultdict(lambda: [0, None, math.inf])
    for (path, comp, prop), (tick, *_rest) in rows:
        g = groups[generic(path, comp, prop)]
        g[0] += 1
        if tick < g[2]:
            g[1], g[2] = path, tick
    print("\nby property (objects, first tick, first object):")
    for name, (count, first, tick) in sorted(groups.items(), key=lambda g: (-g[1][0], g[0]))[:40]:
        print(f"  {count:>4}  {name}  @{tick} {first}")


if __name__ == "__main__":
    main()
