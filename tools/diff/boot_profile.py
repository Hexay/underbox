#!/usr/bin/env python3
"""Profile one engine boot (process start → PERF:START) with `perf record`, split by boot phase.

usage: boot_profile.py attach <stdout-file> <out-dir>     (next to a running `run.sh perf`; see docs/perf-boot.md)
       boot_profile.py report <out-dir> <managed-log> [--top N]

attach waits for the dotnet whose stdout is <stdout-file>, then samples it with task-clock (so a
sample is 1/999 s of thread CPU) until it exits. report cuts the samples at the managed log's
phases: `pre` (start → the Steam web-ticket wait), `wait` (the log's longest silence over 2 s
before PERF:START, client only), `post` (→ PERF:START), and prints per phase the wall time,
CPU per thread class, and the hottest DSOs and symbols. Set DOTNET_PerfMapEnabled=1 on the boot
for managed symbols.
"""
import collections
import os
import re
import subprocess
import sys
import time
from datetime import datetime

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from perf_sample import find_pid, started_at  # noqa: E402

HZ = 999
LOG_TIME = re.compile(r"^(\d{4}/\d\d/\d\d \d\d:\d\d:\d\d\.\d+)\s")
PERF_START = re.compile(r"PERF:START (\d+)")
CLASSES = (
    ("lavapipe", re.compile(r"^(llvmpipe|lp:|lvp|gallivm)")),
    ("mesa disk cache", re.compile(r":disk\$")),
    (".NET JIT", re.compile(r"^\.NET Tiered")),
    (".NET pool", re.compile(r"^\.NET TP Worker")),
    (".NET other", re.compile(r"^\.NET")),
)


def attach(stdout, out):
    os.makedirs(out, exist_ok=True)
    pid = find_pid(stdout, time.time() + 120)
    if pid is None:
        sys.exit("boot_profile: no dotnet writing " + stdout)
    mono, real = time.clock_gettime(time.CLOCK_MONOTONIC), time.time()
    with open(os.path.join(out, "clock.txt"), "w") as f:
        f.write(f"{pid} {started_at(pid)} {real - mono}\n")
    args = ["perf", "record", "-q", "-e", "task-clock", "-F", str(HZ), "-k", "CLOCK_MONOTONIC", "-m", "128",
            "-p", str(pid), "-o", os.path.join(out, "perf.data")]
    if os.environ.get("BOOT_PROFILE_CALLGRAPH"):
        args.insert(2, "-g")
    with open(os.path.join(out, "perf.err"), "w") as err:
        subprocess.run(args, stdout=subprocess.DEVNULL, stderr=err)


def log_times(path):
    times, start = [], None
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            mark = PERF_START.search(line)
            if mark:
                start = int(mark.group(1)) / 1e6
                break
            m = LOG_TIME.match(line)
            if m:
                times.append(datetime.strptime(m.group(1), "%Y/%m/%d %H:%M:%S.%f").timestamp())
    return times, start


def phases(times, started, perf_start):
    """[(name, t0, t1)] in Unix time."""
    gaps = [(b - a, a, b) for a, b in zip(times, times[1:])]
    gap = max(gaps, default=(0, 0, 0))
    if gap[0] < 2:
        return [("boot", started, perf_start)]
    return [("pre", started, gap[1]), ("wait", gap[1], gap[2]), ("post", gap[2], perf_start)]


def classify(comm, tid, pid):
    if tid == pid:
        return "main"
    for name, rx in CLASSES:
        if rx.search(comm):
            return name
    return "unnamed" if comm == "dotnet" else "other: " + comm


def samples(out, pid):
    script = subprocess.run(["perf", "script", "-i", os.path.join(out, "perf.data"), "-F", "tid,comm,time,ip,sym,dso", "--ns"],
                            capture_output=True, text=True, errors="replace").stdout
    rx = re.compile(r"^\s*(.+?)\s+(\d+)\s+(\d+\.\d+):\s+\S+\s+(.*?)\s+\((.*)\)\s*$")
    for line in script.splitlines():
        m = rx.match(line)
        if m:
            comm, tid, t, sym, dso = m.groups()
            yield float(t), classify(comm, int(tid), pid), os.path.basename(dso), sym.split("+0x")[0]


def report(out, log, top):
    pid, started, offset = open(os.path.join(out, "clock.txt")).read().split()
    pid, started, offset = int(pid), float(started), float(offset)
    times, perf_start = log_times(log)
    if perf_start is None:
        sys.exit("boot_profile: no PERF:START in " + log)
    spans = phases(times, started, perf_start)
    by_phase = {n: (collections.Counter(), collections.Counter(), collections.Counter(), collections.Counter()) for n, _, _ in spans}
    seconds = collections.defaultdict(collections.Counter)
    for t, cls, dso, sym in samples(out, pid):
        t += offset
        if started <= t < perf_start:
            seconds[int(t - started)][cls] += 1
        for name, t0, t1 in spans:
            if t0 <= t < t1:
                c, d, s, m = by_phase[name]
                c[cls] += 1
                d[dso] += 1
                s[f"{sym} [{dso}]"] += 1
                if cls == "main":
                    m[f"{sym} [{dso}]"] += 1
    print(f"boot {perf_start - started:.2f} s wall; CPU in s (task-clock samples / {HZ})")
    for name, t0, t1 in spans:
        c, d, s, m = by_phase[name]
        total = sum(c.values()) / HZ
        print(f"\n== {name}: wall {t1 - t0:.2f} s, cpu {total:.2f} s")
        print("  threads: " + ", ".join(f"{k} {v / HZ:.2f}" for k, v in c.most_common()))
        print("  dsos:    " + ", ".join(f"{k} {v / HZ:.2f}" for k, v in d.most_common(top)))
        for label, ctr in (("symbols", s), ("main thread", m)):
            print(f"  {label}:")
            for k, v in ctr.most_common(top):
                print(f"    {v / HZ:7.3f}  {k[:150]}")
    print("\n== CPU s per wall second since process start")
    for s in sorted(seconds):
        print(f"  {s:3d}: " + ", ".join(f"{k} {v / HZ:.2f}" for k, v in seconds[s].most_common(5)))


def main():
    if len(sys.argv) >= 4 and sys.argv[1] == "attach":
        attach(sys.argv[2], sys.argv[3])
    elif len(sys.argv) >= 4 and sys.argv[1] == "report":
        top = int(sys.argv[5]) if len(sys.argv) >= 6 and sys.argv[4] == "--top" else 25
        report(sys.argv[2], sys.argv[3], top)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
