#!/usr/bin/env python3
"""Rank where DRAM read traffic comes from in tools/diff/perf.sh runs taken with RECORD=... .

usage: perf_attrib.py <out-dir> [--by sym|dso] [--top N] [--event NAME]
Reads each <out-dir>/<engine>-<i>/perf.data (perf_events.Recorder, sampling a DRAM fill event over
the window) and sample.json, and prints MB per tick/frame per (thread class, DSO[, symbol]),
median across runs, ours beside real. A sample's period is fill lines, 64 B each. Skid: samples
land a few instructions after the load, so the symbol is right, the instruction may not be.
"""
import argparse
import json
import pathlib
import re
import statistics
import subprocess

LINE = 64
GPU = re.compile(r"^(llvmpipe|lp:|lvp|gallivm|WSI )|:disk\$")


def units(sample):
    """Ticks or frames in the window, from the marks perf_sample.py saw."""
    m = {k: [x for x in sample["marks"] if x["kind"] == k] for k in ("TICK", "END")}
    if m["END"]:
        return int(m["END"][0]["args"][0].split("=")[1])
    ticks = sorted(int(x["args"][0]) for x in m["TICK"])
    return ticks[-1] - ticks[0] if len(ticks) > 1 else 1


def thread_class(comm, main_comm):
    if GPU.search(comm):
        return "lavapipe"
    return "unnamed" if comm == main_comm else "engine"


def rows(data, by, event):
    cmd = ["perf", "report", "-i", str(data), "--stdio", "-q", "-t", "\t", "-F", f"period,comm,pid,dso{',sym' if by == 'sym' else ''}"]
    if event:
        cmd += ["--event", event]
    out = subprocess.run(cmd, capture_output=True, text=True).stdout
    for line in out.splitlines():
        parts = [p.strip() for p in line.split("\t")]
        if len(parts) >= 4 and parts[0].isdigit():
            yield int(parts[0]), parts[1], parts[2], parts[3], parts[4] if len(parts) > 4 else ""


def run_table(run, by, event):
    sample = json.loads((run / "sample.json").read_text())
    n, main_tid = units(sample), str(sample["pid"])
    # perf misses the comm of threads made after it attached; the sampler saw their names.
    names = sample.get("thread_names", {})
    main_comm = names.get(main_tid, "dotnet")
    table = {}
    for period, comm, pid, dso, sym in rows(run / "perf.data", by, event):
        tid = pid.split(":")[0]
        # JIT maps and unsymbolised addresses differ run to run; pool them so medians line up.
        dso = re.sub(r"^\[JIT\] tid \d+$", "[JIT]", dso)
        sym = re.sub(r"^(\[.\]) 0x[0-9a-f]+$", r"\1 ?", sym)
        cls = "main" if tid == main_tid else thread_class(names.get(tid, comm), main_comm)
        key = (cls, dso, sym)
        table[key] = table.get(key, 0) + period * LINE / 1e6 / n
    return table


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--by", default="sym", choices=("sym", "dso"))
    ap.add_argument("--top", type=int, default=30)
    ap.add_argument("--event", default="")
    a = ap.parse_args()
    per = {}
    for run in sorted(pathlib.Path(a.out).glob("*-*/perf.data")):
        if not (run.parent / "sample.json").exists():
            continue
        eng = run.parent.name.rsplit("-", 1)[0]
        per.setdefault(eng, []).append(run_table(run.parent, a.by, a.event))
    med = {}
    for eng, tables in per.items():
        keys = set().union(*tables)
        med[eng] = {k: statistics.median(t.get(k, 0) for t in tables) for k in keys}
    engines = [e for e in ("ours", "real") if e in med]
    total = {e: sum(med[e].values()) for e in engines}
    print(f"DRAM read MB per tick/frame by thread class, DSO{', symbol' if a.by == 'sym' else ''}; "
          f"medians over runs ({', '.join(f'{e} {len(per[e])}' for e in engines)})")
    print("total " + ", ".join(f"{e} {total[e]:.2f}" for e in engines))
    keys = sorted(set().union(*(med[e] for e in engines)), key=lambda k: -max(med[e].get(k, 0) for e in engines))
    print(f"{'class':9}{'dso':28}{'symbol':60}" + "".join(f"{e:>9}" for e in engines))
    for k in keys[:a.top]:
        print(f"{k[0]:9}{k[1][:27]:28}{k[2][:59]:60}" + "".join(f"{med[e].get(k, 0):9.3f}" for e in engines))


if __name__ == "__main__":
    main()
