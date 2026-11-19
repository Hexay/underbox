#!/usr/bin/env python3
"""Sample one engine process's per-thread CPU and memory while tools/diff/PerfMark.cs logs its markers.

usage: perf_sample.py <stdout-file> <log-file|@file-naming-the-log> <out.json> [--period s] [--gpu regex]

Finds the dotnet process whose stdout is <stdout-file>, then polls every thread's
/proc/<pid>/task/<tid>/schedstat (ns of CPU, not wall time) and VmRSS/VmHWM until PERF:DONE or exit.
Threads whose name matches --gpu (lavapipe's workers) are summed apart as the GPU-work proxy.
CPU at a marker is interpolated from the samples at the marker's own timestamp, so log buffering
doesn't skew it. See docs/differential-perf.md.
"""
import argparse
import bisect
import json
import os
import re
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from perf_events import EVENTS, Counters, Recorder  # noqa: E402

MARK = re.compile(r"PERF:(START|TICK|BEGIN|END|DONE)\b(.*)")
# Threads.totals' order; each marker gets its CPU in every series.
SERIES = ("eng", "gpu", "main", "unnamed")
# Threads.fills' order: DRAM fill counts (perf_events.EVENTS) per thread class, and the whole process.
FILLS = tuple(f"{e}_{c}" for e in EVENTS for c in ("eng", "gpu", "all"))
FILL_EVERY = 10


def find_pid(stdout, deadline):
    want = os.path.realpath(stdout)
    while time.time() < deadline:
        for pid in filter(str.isdigit, os.listdir("/proc")):
            try:
                if os.readlink(f"/proc/{pid}/fd/1") == want and read(f"/proc/{pid}/comm").strip() == "dotnet":
                    return int(pid)
            except OSError:
                continue
        time.sleep(0.05)
    return None


def read(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read()


def started_at(pid):
    """The process's start as Unix time: /proc/<pid>/stat starttime is in clock ticks since boot."""
    fields = read(f"/proc/{pid}/stat").rsplit(")", 1)[1].split()
    btime = next(int(l.split()[1]) for l in read("/proc/stat").splitlines() if l.startswith("btime "))
    return btime + int(fields[19]) / os.sysconf("SC_CLK_TCK")


class Threads:
    """Cumulative CPU per thread; a thread that exits keeps its last reading."""

    def __init__(self, pid, gpu):
        self.pid, self.gpu = pid, gpu
        self.fds, self.cpu, self.name = {}, {}, {}
        self.counters = Counters()

    def sample(self, refresh_names):
        try:
            tids = os.listdir(f"/proc/{self.pid}/task")
        except OSError:
            return False
        if self.counters.ok:
            self.counters.add(tids)
        for tid in tids:
            if tid not in self.fds:
                try:
                    self.fds[tid] = os.open(f"/proc/{self.pid}/task/{tid}/schedstat", os.O_RDONLY)
                except OSError:
                    continue
            if refresh_names or tid not in self.name:
                try:
                    self.name[tid] = read(f"/proc/{self.pid}/task/{tid}/comm").strip()
                except OSError:
                    pass
            try:
                self.cpu[tid] = int(os.pread(self.fds[tid], 64, 0).split()[0])
            except (OSError, ValueError, IndexError):
                os.close(self.fds.pop(tid))
        return True

    def kind(self, tid):
        """`main`, `unnamed` (a thread that kept the process's name: lavapipe's compute pool), or
        `named`."""
        if tid == str(self.pid):
            return "main"
        return "unnamed" if self.name.get(tid) == self.name.get(str(self.pid)) else "named"

    def totals(self):
        """Engine and GPU-proxy CPU (ns), and the main and unnamed threads' share of engine."""
        eng = gpu = main = unnamed = 0
        for tid, ns in self.cpu.items():
            if self.gpu.search(self.name.get(tid, "")):
                gpu += ns
                continue
            eng += ns
            kind = self.kind(tid)
            main += ns if kind == "main" else 0
            unnamed += ns if kind == "unnamed" else 0
        return eng, gpu, main, unnamed

    def fills(self):
        """DRAM fills in FILLS' order: per-thread counts split engine/lavapipe, then the whole process."""
        out = dict.fromkeys(FILLS, 0)
        for tid, counts in self.counters.per_thread().items():
            cls = "gpu" if self.gpu.search(self.name.get(tid, "")) else "eng"
            for e, v in counts.items():
                out[f"{e}_{cls}"] += v
        for e, v in self.counters.whole().items():
            out[f"{e}_all"] = v
        return tuple(out[k] for k in FILLS)

    def by_name(self, values=None):
        out = {}
        for tid, ns in (self.cpu if values is None else values).items():
            kind = self.kind(tid)
            key = f"{self.name.get(tid, '?')} ({kind})" if kind != "named" else re.sub(r"\d+$", "#", self.name.get(tid, "?"))
            out[key] = out.get(key, 0) + ns
        return out


class Log:
    def __init__(self, spec):
        self.spec, self.path, self.pos, self.buf = spec, None, 0, ""

    def lines(self):
        if self.path is None:
            try:
                self.path = read(self.spec[1:]).strip() if self.spec.startswith("@") else self.spec
            except OSError:
                return []
        try:
            with open(self.path, "rb") as f:
                f.seek(self.pos)
                data = f.read()
        except OSError:
            return []
        self.pos += len(data)
        self.buf += data.decode("utf-8", "replace")
        *done, self.buf = self.buf.split("\n")
        return done


def memory(pid):
    out = {}
    for line in read(f"/proc/{pid}/status").splitlines():
        if line.startswith(("VmRSS:", "VmHWM:")):
            out[line[:5]] = int(line.split()[1]) * 1024
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("stdout")
    ap.add_argument("log")
    ap.add_argument("out")
    ap.add_argument("--period", type=float, default=0.001)
    ap.add_argument("--gpu", default=r"^(llvmpipe|lp:|lvp|gallivm|WSI )|:disk\$")
    ap.add_argument("--wait", type=float, default=600, help="seconds to wait for the process")
    ap.add_argument("--record", default="", help="perf record args over the window (perf_events.Recorder)")
    a = ap.parse_args()
    pid = find_pid(a.stdout, time.time() + a.wait)
    if pid is None:
        sys.exit(f"perf_sample: no dotnet process writing {a.stdout}")
    threads, log = Threads(pid, re.compile(a.gpu)), Log(a.log)
    recorder = None
    start = started_at(pid)
    ts, rss, marks, fts = [], [], [], []
    series = {k: [] for k in SERIES + FILLS}
    hwm, n, done_at, window_names, window_fills = 0, 0, None, None, None
    while threads.sample(n % 200 == 0):
        now = time.time()
        ts.append(now)
        for k, v in zip(SERIES, threads.totals()):
            series[k].append(v)
        if n % FILL_EVERY == 0 and threads.counters.ok:
            fts.append(now)
            for k, v in zip(FILLS, threads.fills()):
                series[k].append(v)
        if n % 20 == 0:
            try:
                m = memory(pid)
                hwm = max(hwm, m.get("VmHWM", 0))
                rss.append((now, m.get("VmRSS", 0)))
            except OSError:
                break
        for line in log.lines():
            hit = MARK.search(line)
            if hit:
                marks.append({"kind": hit[1], "args": hit[2].split(), "seen": now})
                # Attached at scene start, not at boot: perf only names the DSOs mapped when it attaches.
                if hit[1] == "START" and a.record:
                    recorder = Recorder(pid, a.record, os.path.dirname(os.path.abspath(a.out)))
                if hit[1] == "BEGIN" or (hit[1] == "TICK" and hit[2].split()[0] == "1"):
                    window_names, window_fills = threads.by_name(), threads.by_name(l2_by_tid(threads))
                    if recorder:
                        recorder.enable()
                if hit[1] in ("DONE", "END") and recorder:
                    recorder.stop()
                    recorder = None
                if hit[1] == "DONE":
                    done_at = now
        if done_at and now - done_at > 0.5:
            break
        n += 1
        time.sleep(a.period)
    if recorder:
        recorder.stop()
    names = window_delta(window_names, threads.by_name(), 1e9)
    fills = window_delta(window_fills, threads.by_name(l2_by_tid(threads)), 1e6 / 64)
    for m in marks:
        if m["args"] and m["args"][-1].isdigit() and ts:
            m["t"] = int(m["args"][-1]) / 1e6
            m.update({k: cpu_at(ts, series[k], m["t"]) for k in SERIES})
            if fts:
                m.update({k: cpu_at(fts, series[k], m["t"]) for k in FILLS})
    with open(a.out + ".tmp", "w") as f:
        json.dump({
            "pid": pid, "started": start, "samples": len(ts),
            "period": (ts[-1] - ts[0]) / max(1, len(ts) - 1) if ts else 0,
            "last": [ts[-1], series["eng"][-1], series["gpu"][-1]] if ts else None, "rss": rss, "hwm": hwm, "marks": marks,
            "threads_window_s": names, "threads_window_dram_mb": fills,
            "counter_min_running": threads.counters.min_running, "thread_names": threads.name,
        }, f)
    os.replace(a.out + ".tmp", a.out)


def l2_by_tid(threads):
    return {tid: c.get("l2_dram", 0) for tid, c in threads.counters.last.items()}


def window_delta(before, after, unit):
    """Per-name growth from the window's first marker to the end, top 40, in `unit`s."""
    d = {k: (v - (before or {}).get(k, 0)) / unit for k, v in after.items()}
    return dict(sorted(d.items(), key=lambda kv: -kv[1])[:40])


def cpu_at(ts, values, t):
    """One series' CPU (ns) at Unix time t, linear between the samples around it."""
    i = bisect.bisect_left(ts, t)
    if i <= 0:
        return values[0]
    if i >= len(ts):
        return values[-1]
    f = (t - ts[i - 1]) / (ts[i] - ts[i - 1])
    return values[i - 1] + f * (values[i] - values[i - 1])


if __name__ == "__main__":
    main()
