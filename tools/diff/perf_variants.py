#!/usr/bin/env python3
"""Table of tools/diff/perf-variants.sh's runs: per variant, medians [min-max], and the median of its
change against the first variant's run in the same round (paired, so load drift between rounds cancels).

usage: perf_variants.py <out-dir> <label>=[...] ...
"""
import json
import pathlib
import statistics
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from perf_report import metrics  # noqa: E402

COLUMNS = ("dram read MB/frame", "dram lavapipe MB/frame", "dram engine MB/frame", "frame engine cpu ms",
           "frame lavapipe cpu ms", "frame main thread ms", "dram per-thread coverage")


def runs(out, label):
    """{round: metrics} of one variant's runs that reached the window."""
    found = {}
    for d in sorted(out.glob(f"{label}-*/sample.json")):
        name, _, rnd = d.parent.name.rpartition("-")
        if name == label:
            m = metrics(json.loads(d.read_text()), "client")
            if "dram read MB/frame" in m:
                found[rnd] = m
    return found


def main():
    out = pathlib.Path(sys.argv[1])
    labels = [a.split("=", 1)[0] for a in sys.argv[2:]]
    table = {label: runs(out, label) for label in labels}
    base = table[labels[0]]
    print("variant | runs | " + " | ".join(COLUMNS) + "   (cell: median [min-max] (paired change against " + labels[0] + "))")
    for label in labels:
        cells = []
        for c in COLUMNS:
            vals = [m[c] for m in table[label].values() if c in m]
            if not vals:
                cells.append("-")
                continue
            deltas = [m[c] - base[r][c] for r, m in table[label].items() if c in m and r in base and c in base[r]]
            delta = f" ({statistics.median(deltas):+.1f})" if deltas and label != labels[0] else ""
            cells.append(f"{statistics.median(vals):.3g} [{min(vals):.3g}-{max(vals):.3g}]{delta}")
        print(f"{label} | {len(table[label])} | " + " | ".join(cells))


if __name__ == "__main__":
    main()
