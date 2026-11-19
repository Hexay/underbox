"""Per-thread hardware counters through perf_event_open(2), for tools/diff/perf_sample.py.

Counts DRAM read fills per thread (docs/differential-perf.md, "Memory bandwidth"). Works at
perf_event_paranoid <= 1 on our own processes; no root, no perf binary.
"""
import ctypes
import os
import shlex
import signal
import struct
import subprocess
import time

_libc = ctypes.CDLL(None, use_errno=True)
_SYS_PERF_EVENT_OPEN = 298  # x86_64
_FD_CLOEXEC = 8
_TIME_ENABLED, _TIME_RUNNING = 1, 2
_INHERIT = 1 << 1

# name -> (pmu type, config); `perf stat -vv -e <name> true` prints these. AMD Zen 5 core PMU.
EVENTS = {
    "l2_dram": (4, 0x100000865),  # l2_fill_rsp_src.dram_io_near: L2 fills from DRAM, demand + L2 prefetch
    "l1_dram": (4, 0x4844),  # ls_any_fills_from_sys.dram_io_all: L1D fills (demand + L1 prefetch) from DRAM
}
LINE = 64


def _attr(pmu, config, inherit):
    # perf_event_attr up to PERF_ATTR_SIZE_VER5 (112 bytes); everything after read_format is zero
    # except the flags word.
    head = struct.pack("=IIQQQQQ", pmu, 112, config, 0, 0, _TIME_ENABLED | _TIME_RUNNING, _INHERIT if inherit else 0)
    return ctypes.create_string_buffer(head + bytes(112 - len(head)), 112)


def open_event(name, tid, inherit=False):
    """An fd counting `name` on thread `tid` (and, with inherit, every thread it creates later)."""
    pmu, config = EVENTS[name]
    fd = _libc.syscall(_SYS_PERF_EVENT_OPEN, _attr(pmu, config, inherit), int(tid), -1, -1, _FD_CLOEXEC)
    if fd < 0:
        raise OSError(ctypes.get_errno(), f"perf_event_open {name} on {tid}")
    return fd


def read_event(fd):
    """(count scaled for multiplexing, fraction of time the counter was on the PMU)."""
    value, enabled, running = struct.unpack("=QQQ", os.read(fd, 24))
    if not running:
        return 0, 1.0
    return value * enabled / running, running / enabled


class Recorder:
    """`perf record -p <pid> <args>` into <dir>/perf.data, events off until `enable` (the window's
    first marker) and back off at `stop`. Attaching inherits to threads created later."""

    def __init__(self, pid, args, out_dir):
        ctl = os.path.join(out_dir, "perf.ctl")
        if os.path.exists(ctl):
            os.unlink(ctl)
        os.mkfifo(ctl)
        self.ctl = os.open(ctl, os.O_RDWR)
        self.proc = subprocess.Popen(
            ["perf", "record", "-p", str(pid), "-D", "-1", "--control", f"fifo:{ctl}",
             "-o", os.path.join(out_dir, "perf.data"), *shlex.split(args)],
            stdout=open(os.path.join(out_dir, "perf-record.log"), "w"), stderr=subprocess.STDOUT)
        self.on = False

    def enable(self):
        if not self.on:
            os.write(self.ctl, b"enable\n")
            self.on = True

    def stop(self):
        if self.on:
            os.write(self.ctl, b"disable\n")
        time.sleep(0.2)
        self.proc.send_signal(signal.SIGINT)
        try:
            self.proc.wait(timeout=60)
        except subprocess.TimeoutExpired:
            self.proc.kill()


class Counters:
    """EVENTS on one process, two ways (validation: docs/differential-perf.md):

    - `own`: a non-inherited fd per thread, opened when the sampler first sees the thread; the
      per-thread split. Misses a thread's life before it is seen, and threads too short to be seen.
    - `tree`: an inherited fd on every thread alive at the first `add`; every thread created later
      is a descendant of one of them, so their sum is the whole process from that moment on.
    """

    def __init__(self):
        self.own, self.tree, self.last, self.min_running = {}, [], {}, 1.0
        self.ok = True

    def add(self, tids):
        first = not self.own and not self.tree
        for tid in tids:
            if tid in self.own:
                continue
            try:
                self.own[tid] = {n: open_event(n, tid) for n in EVENTS}
                if first:
                    self.tree.append({n: open_event(n, tid, inherit=True) for n in EVENTS})
            except OSError as e:
                self.own.setdefault(tid, {})
                if first and self.ok:
                    print(f"perf_events: {e}; no DRAM counters", flush=True)
                    self.ok = False

    def _read(self, fds):
        out = {}
        for n, fd in fds.items():
            try:
                out[n], running = read_event(fd)
                self.min_running = min(self.min_running, running)
            except OSError:
                pass
        return out

    def per_thread(self):
        """tid -> {event: count}; an exited thread keeps its last reading."""
        for tid, fds in self.own.items():
            self.last[tid] = {**self.last.get(tid, {}), **self._read(fds)}
        return self.last

    def whole(self):
        """{event: count} over the process tree since the first `add`."""
        out = dict.fromkeys(EVENTS, 0)
        for fds in self.tree:
            for n, v in self._read(fds).items():
                out[n] += v
        return out
