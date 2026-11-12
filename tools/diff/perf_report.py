#!/usr/bin/env python3
"""Summarise tools/diff/perf.sh's runs: per-engine median and spread of each metric, ours/real ratio.

usage: perf_report.py <out-dir> <server|client> [setup note]
Reads <out-dir>/<engine>-<i>/sample.json (tools/diff/perf_sample.py) and <out-dir>/load.txt.
"""
import json
import pathlib
import statistics
import sys

NOISY = 0.25


def pct(values, q):
    s = sorted(values)
    return s[min(len(s) - 1, int(round(q * (len(s) - 1))))]


def marks(sample, kind):
    return [m for m in sample["marks"] if m["kind"] == kind and "t" in m]


def steady_rss(sample, t0, t1):
    inside = [r for t, r in sample["rss"] if t0 <= t <= t1]
    return statistics.median(inside) if inside else None


def metrics(sample, side):
    """One run's numbers; times in ms, memory in MiB. Missing markers leave a metric out."""
    out = {}
    start = marks(sample, "START")
    if start:
        out["boot wall s"] = (start[0]["t"] - sample["started"])
        out["boot cpu s"] = (start[0]["eng"] + start[0]["gpu"]) / 1e9
    out["peak rss MiB"] = sample["hwm"] / 2**20
    t0 = t1 = None
    if side == "server":
        ticks = sorted(marks(sample, "TICK"), key=lambda m: int(m["args"][0]))
        if len(ticks) > 1:
            per = [(b["eng"] + b["gpu"] - a["eng"] - a["gpu"]) / 1e6 for a, b in zip(ticks, ticks[1:])]
            out["tick cpu median ms"] = statistics.median(per)
            out["tick cpu p95 ms"] = pct(per, 0.95)
            out["tick cpu total s"] = sum(per) / 1e3
            out["ticks"] = len(per)
            out["tick wall ms"] = (ticks[-1]["t"] - ticks[0]["t"]) * 1e3 / len(per)
            t0, t1 = ticks[0]["t"], ticks[-1]["t"]
    else:
        begin, end = marks(sample, "BEGIN"), marks(sample, "END")
        if begin and end:
            b, e = begin[0], end[0]
            n = int(e["args"][0].split("=")[1])
            out["frame engine cpu ms"] = (e["eng"] - b["eng"]) / 1e6 / n
            out["frame lavapipe cpu ms"] = (e["gpu"] - b["gpu"]) / 1e6 / n
            out["frame wall ms"] = (e["t"] - b["t"]) * 1e3 / n
            out["frames"] = n
            t0, t1 = b["t"], e["t"]
    if t0 is not None:
        rss = steady_rss(sample, t0, t1)
        if rss:
            out["steady rss MiB"] = rss / 2**20
    lags = [m["seen"] - m["t"] for m in sample["marks"] if "t" in m]
    if lags:
        out["marker lag ms"] = statistics.median(lags) * 1e3
    out["sample period ms"] = sample["period"] * 1e3
    return out


def load_avg(path):
    rows = [list(map(float, l.split())) for l in path.read_text().splitlines() if l.strip()]
    return rows[0], rows[-1]


def fmt(v):
    return "-" if v is None else f"{v:.3g}" if abs(v) < 1000 else f"{v:.0f}"


def main():
    out, side = pathlib.Path(sys.argv[1]), sys.argv[2]
    note = sys.argv[3] if len(sys.argv) > 3 else ""
    runs, threads = {}, {}
    for f in sorted(out.glob("*-*/sample.json")):
        eng = f.parent.name.rsplit("-", 1)[0]
        sample = json.loads(f.read_text())
        runs.setdefault(eng, []).append(metrics(sample, side))
        for name, s in sample.get("threads_window_s", {}).items():
            threads.setdefault(eng, {}).setdefault(name, []).append(s)
    (l0, l1) = load_avg(out / "load.txt")
    noisy = abs(l1[0] - l0[0]) > NOISY * max(l0[0], 1)
    print(f"perf {out.name} ({side}); {note}")
    print(f"load average start {l0[0]:.1f} {l0[1]:.1f} {l0[2]:.1f}, end {l1[0]:.1f} {l1[1]:.1f} {l1[2]:.1f}"
          + ("  ** NOISY: 1-min load moved more than 25% **" if noisy else ""))
    print("CPU is thread CPU time (schedstat), not wall time. On lavapipe it measures CPU cost, not GPU frame time.")
    engines = [e for e in ("ours", "real") if e in runs]
    keys = []
    for e in engines:
        for r in runs[e]:
            keys += [k for k in r if k not in keys]
    head = f"{'metric':24}" + "".join(f"{e + ' median [min-max]':>30}" for e in engines)
    print("\n" + head + ("    ours/real" if len(engines) == 2 else ""))
    for k in keys:
        row, meds = f"{k:24}", []
        for e in engines:
            vals = [r[k] for r in runs[e] if k in r]
            if not vals:
                row += f"{'-':>30}"; meds.append(None); continue
            m = statistics.median(vals)
            meds.append(m)
            row += f"{fmt(m) + ' [' + fmt(min(vals)) + '-' + fmt(max(vals)) + ']':>30}"
        if len(meds) == 2 and meds[0] is not None and meds[1]:
            row += f"{meds[0] / meds[1]:13.2f}"
        print(row)
    print("\nruns: " + ", ".join(f"{e} {len(runs[e])}" for e in engines))
    for e in engines:
        top = sorted(((statistics.median(v), n) for n, v in threads.get(e, {}).items()), reverse=True)[:8]
        print(f"{e} busiest threads in the window (CPU s, median): " + ", ".join(f"{n} {s:.2f}" for s, n in top))


if __name__ == "__main__":
    main()
