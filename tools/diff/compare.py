"""Compare scenario markers from two server logs (ours vs real engine2) and rank divergences.

usage: python3 -I tools/diff/compare.py <ours.log> <real.log> [marker-regex=MATCH:|GAME:|DIFF:] [tol=0.5]

A marker's shape is its text with every number replaced by '#'. Markers pair up by shape and
occurrence index; numbers in a pair are compared with an absolute tolerance. Severity:
  high   a shape present on one engine only (an event happened on one side)
  medium paired numbers differ by more than tol (positions, counts)
  low    same multiset of shapes in a different order (event timing)
Exit status: 0 when nothing diverges, 1 otherwise.
"""
import difflib
import re
import sys
from collections import Counter, defaultdict

NUM = re.compile(r"-?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?")


def markers(path, pattern):
    rx = re.compile(f"({pattern}).*")
    out = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            m = rx.search(line.rstrip("\n"))
            if m:
                out.append(m.group(0).strip())
    return out


def shape(text):
    return NUM.sub("#", text)


def by_shape(ms):
    groups = defaultdict(list)
    for m in ms:
        groups[shape(m)].append(m)
    return groups


def compare(ours, real, tol):
    found = []
    so, sr = Counter(map(shape, ours)), Counter(map(shape, real))
    for s in sorted(set(so) | set(sr)):
        if so[s] != sr[s]:
            found.append(("high", f"count {so[s]} ours vs {sr[s]} real: {s}"))
    go, gr = by_shape(ours), by_shape(real)
    for s in sorted(set(go) & set(gr)):
        for i, (a, b) in enumerate(zip(go[s], gr[s])):
            na, nb = [float(x) for x in NUM.findall(a)], [float(x) for x in NUM.findall(b)]
            worst = max((abs(x - y) for x, y in zip(na, nb)), default=0.0)
            if worst > tol:
                found.append(("medium", f"delta {worst:.3g} (#{i}): ours `{a}` real `{b}`"))
    if so == sr:
        seq_o, seq_r = list(map(shape, ours)), list(map(shape, real))
        if seq_o != seq_r:
            ops = [o for o in difflib.SequenceMatcher(a=seq_o, b=seq_r).get_opcodes() if o[0] != "equal"]
            for tag, i1, i2, j1, j2 in ops[:5]:
                found.append(("low", f"order {tag}: ours {seq_o[i1:i2]} real {seq_r[j1:j2]}"))
    return found


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    pattern = sys.argv[3] if len(sys.argv) > 3 else "MATCH:|GAME:|DIFF:"
    tol = float(sys.argv[4]) if len(sys.argv) > 4 else 0.5
    ours, real = markers(sys.argv[1], pattern), markers(sys.argv[2], pattern)
    print(f"== markers: ours {len(ours)}, real {len(real)}")
    found = compare(ours, real, tol)
    rank = {"high": 0, "medium": 1, "low": 2}
    for sev, what in sorted(found, key=lambda f: rank[f[0]]):
        print(f"{sev.upper():6} {what[:220]}")
    if not ours or not real:
        print("HIGH   one side logged no markers: the run itself failed")
        found.append(("high", "empty"))
    print(f"DIFF: {len(found)} divergence(s)")
    sys.exit(1 if found else 0)


if __name__ == "__main__":
    main()
