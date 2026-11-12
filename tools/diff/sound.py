#!/usr/bin/env python3
"""Compare two corpus/sound captures (interleaved f32 LE stereo at 44100 Hz, SDL's disk driver).

usage: sound.py <ours.raw> <real.raw>   or   sound.py <one.raw>   (just measure it)

Each capture is aligned on its own first onset, the start of the schedule in
corpus/sound/Code/SoundScript.cs. Per event: onset relative to the schedule, steady-state RMS per
channel, L/R balance, dominant frequency (interpolated zero crossings), the level of the tail that
follows the sound (reverb), and for the loop its audible length. Divergences are ranked by size.
See docs/differential-audio.md.
"""
import math
import sys
from array import array

RATE = 44100
# (name, scheduled start s, length s): keep in step with SoundScript.cs.
EVENTS = [
    ("ui", 0.0, 0.5),
    ("left200", 1.0, 0.5),
    ("right200", 2.0, 0.5),
    ("ahead200", 3.0, 0.5),
    ("ahead800", 4.0, 0.5),
    ("vol0.5", 5.0, 0.5),
    ("pitch1.5", 6.0, 0.5 / 1.5),
    ("noise", 7.0, 0.3),
    ("loop", 8.0, 1.5),
]
# Score divisors: what counts as one unit of divergence per metric.
UNITS = {"db_l": 1.0, "db_r": 1.0, "balance": 1.0, "hz": 2.0, "onset_ms": 5.0, "tail_db": 3.0, "length_ms": 30.0, "dropouts": 1.0}
FLOOR_DB = -120.0


def load(path):
    a = array("f")
    with open(path, "rb") as f:
        a.frombytes(f.read())
    if sys.byteorder != "little":
        a.byteswap()
    return a[0::2], a[1::2]


def db(x):
    return 20 * math.log10(x) if x > 1e-6 else FLOOR_DB


def rms(x, a, b):
    a, b = max(a, 0), min(b, len(x))
    return math.sqrt(sum(v * v for v in x[a:b]) / (b - a)) if b > a else 0.0


def first_above(l, r, a, b, level):
    for i in range(max(a, 0), min(b, len(l))):
        if abs(l[i]) > level or abs(r[i]) > level:
            return i
    return None


def last_above(l, r, a, b, level):
    for i in range(min(b, len(l)) - 1, max(a, 0) - 1, -1):
        if abs(l[i]) > level or abs(r[i]) > level:
            return i
    return None


def peak(l, r, a, b):
    a, b = max(a, 0), min(b, len(l))
    return max((max(abs(l[i]), abs(r[i])) for i in range(a, b)), default=0.0)


def dropouts(l, r, a, b, min_run=64):
    """Runs of exact digital silence inside a sounding span: the device ran dry (an underrun)."""
    n = run = 0
    for i in range(max(a, 0), min(b, len(l))):
        if l[i] == 0.0 and r[i] == 0.0:
            run += 1
        else:
            n += run >= min_run
            run = 0
    return n


def frequency(x, a, b):
    """Mean frequency from linearly interpolated upward zero crossings in x[a:b]."""
    ups = []
    for i in range(max(a, 1), min(b, len(x))):
        if x[i - 1] < 0 <= x[i]:
            ups.append(i - 1 + -x[i - 1] / (x[i] - x[i - 1]))
    return (len(ups) - 1) * RATE / (ups[-1] - ups[0]) if len(ups) > 2 else 0.0


def bursts(l, r, level=0.05, hop=441, quiet_s=0.3):
    """(start, end) sample spans where either channel's block peak exceeds `level`, joined across
    gaps shorter than `quiet_s` (the schedule leaves at least 0.5 s between sounds). `level` sits
    above real's menu blip (about -48 dB) and below the quietest event (ahead800, -20 dB)."""
    spans = []
    for a in range(0, len(l), hop):
        if peak(l, r, a, a + hop) > level:
            if spans and a - spans[-1][1] < quiet_s * RATE:
                spans[-1][1] = a + hop
            else:
                spans.append([a, a + hop])
    return spans


def measure(path):
    """Events matched to bursts in order, so a device underrun between sounds delays one onset
    instead of shifting every later window."""
    l, r = load(path)
    spans = bursts(l, r)
    if not spans:
        return None, len(l) / RATE
    out = {"bursts": len(spans)}
    prev = None
    for i, (name, start, length) in enumerate(EVENTS):
        if i >= len(spans):
            out[name] = {"missing": 1}
            continue
        a, b = spans[i][0] - 441, spans[i][1]
        nxt = spans[i + 1][0] if i + 1 < len(spans) else len(l)
        p = peak(l, r, a, b)
        on = first_above(l, r, a, b, 0.1 * p)
        end = last_above(l, r, on, b, 0.1 * p)
        # Steady state: skip the ramp and the first block's smoothing.
        m0, m1 = on + int(0.05 * RATE), on + int(min(0.9 * length, 0.4) * RATE)
        rl, rr = rms(l, m0, m1), rms(r, m0, m1)
        mid = [l[k] + r[k] for k in range(m0, m1)]
        expected = (start - prev[1]) * RATE if prev else 0
        e = {
            "onset_ms": ((on - prev[0] - expected) if prev else 0) * 1000 / RATE,
            "db_l": db(rl),
            "db_r": db(rr),
            "balance": db(rr) - db(rl) if rl > 1e-6 and rr > 1e-6 else 0.0,
            "hz": frequency(mid, 0, len(mid)) if name != "noise" else 0.0,
            "tail_db": db(rms(l, on + int((length + 0.1) * RATE), nxt - int(0.05 * RATE))) if name != "loop" else 0.0,
            "dropouts": dropouts(l, r, on, end),
        }
        if name == "loop":
            e["length_ms"] = (end - on) * 1000 / RATE
        out[name] = e
        prev = (on, start)
    return out, len(l) / RATE


def show(name, e):
    if "missing" in e:
        return f"{name:9} missing"
    extra = f" length {e['length_ms']:7.1f} ms" if "length_ms" in e else ""
    extra += f" dropouts {e['dropouts']}" if e["dropouts"] else ""
    return (f"{name:9} onset {e['onset_ms']:+7.1f} ms  L {e['db_l']:7.2f} dB  R {e['db_r']:7.2f} dB  "
            f"bal {e['balance']:+6.2f} dB  f {e['hz']:7.1f} Hz  tail {e['tail_db']:7.1f} dB{extra}")


def main(argv):
    if len(argv) not in (2, 3):
        print(__doc__)
        return 2
    runs = [(p, *measure(p)) for p in argv[1:]]
    for path, ev, secs in runs:
        if ev is None:
            print(f"== {path}: {secs:.1f} s\nRESULT: FAIL (silent capture)")
            return 1
        print(f"== {path}: {secs:.1f} s, {ev['bursts']} bursts for {len(EVENTS)} events")
        for name, *_ in EVENTS:
            print("  " + show(name, ev[name]))
    if len(runs) == 1:
        return 0
    ours, real = runs[0][1], runs[1][1]
    rows = []
    for name, *_ in EVENTS:
        a, b = ours[name], real[name]
        if ("missing" in a) != ("missing" in b):
            rows.append((math.inf, name, "event", "ours" if "missing" in b else "real", ""))
            continue
        for k, unit in UNITS.items():
            if k in a and k in b and "missing" not in a:
                d = a[k] - b[k]
                rows.append((abs(d) / unit, name, k, f"{a[k]:.2f}", f"{b[k]:.2f} (ours-real {d:+.2f})"))
    rows.sort(key=lambda r: -r[0])
    print("== divergences, largest first (score = |ours-real| / unit)")
    for score, name, k, a, b in rows:
        if score >= 1:
            print(f"  {score:7.1f}  {name:9} {k:9} ours {a}  real {b}")
    worst = rows[0][0] if rows else 0
    print(f"RESULT: {'PASS' if worst < 1 else 'DIVERGES'} (worst score {worst:.1f})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
