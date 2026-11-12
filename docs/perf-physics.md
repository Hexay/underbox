# Server tick profile (pile)

Part of docs/differential-perf.md. Scenario `pile` (328 dynamic bodies, headless dedicated server,
`taskset 0-7`), 2026-11-12, shared box at load 30–60.

## Tools

| What | How |
| --- | --- |
| Tick CPU, ours vs real | `tools/diff/run.sh perf pile both` |
| A/B of two builds | `UNDERBOX_ENGINE2=<so> tools/diff/run.sh perf pile ours`, alternating the builds run by run |
| Profile the server | `perf record -F 1999 -p <dotnet pid>` during the ticks, `DOTNET_PerfMapEnabled=1` for managed frames; `perf stat -p` for counters |
| Physics alone | `cargo run --release -p physics --example pile [ticks] [warmup]`: the same pile in a bare `World`, per-step time, CPU of all threads, a hash of every body's final state and of every contact event |
| Allocations | `heaptrack` on the example at two tick counts; the difference is per tick |

`perf_event_paranoid=1` is enough for all of these, no root needed. Release `libengine2.so` keeps
its symbol table, but `--call-graph dwarf` doesn't unwind through it: use flat profiles.

## Ranking (before)

Share of the server process's samples during the measured ticks (all threads):

| Area | Share |
| --- | --- |
| Frame pacer: `SDL_DelayPrecise` spun a full 1 ms each frame | 19.7% |
| Rapier solver | 16.1% |
| Managed code (same on both engines) | 15.3% |
| .NET tiered JIT thread | 14.3% |
| Our contact, persist, bounce and sleep code (`contacts.rs` sort above all) | 8.8% |
| SipHash on handles (physics maps, the `objects` registry) | 6.9% |
| CLR runtime | 5.4% |
| Touch re-measure (`contacts::touches`, EPA) | 3.8% |
| Interop: `objects::get`, `OnActive` marshalling | 3.4% |
| Rapier narrow phase, broad phase and islands; malloc | 2.7%, 1.6%, 1.2% |

Physics alone (the example) took 1.34 ms a step, half of it our wrapper: SipHash 21%, the contact
diff 20%, the touch re-measure 16%. It made 1016 allocations a step. CCD and the query pipeline
didn't show: queries are views over the broad phase, with no update of their own.

## Changes

- **Frame pacer:** `globalSdl.DelayPrecise` calls SDL's own `SDL_DelayPrecise` (sleep in 1 ms steps,
  then the rest less the worst overshoot), not a sleep plus a 1 ms spin.
- **Contacts:** touching pairs are a Vec sorted by creation rank (computed once per pair), diffed
  by binary search, its buffer reused. Before: a HashMap rebuilt each step and sorted twice with a
  hash lookup per comparison.
- **Touch re-measure:** pairs whose shapes overlap (GJK `intersection_test`) touch without running
  the contact query's EPA. Gotcha: only overlaps take the shortcut. For box pairs parry's contact
  query and GJK `distance` disagree both ways near 0.1 (0.24 apart reads as touching; one pair 0.004
  apart doesn't), and that changed events.
- **Hashing:** `rustc-hash` (Fx) for the physics crate's per-body and per-pair maps and the
  `objects` registry. Deterministic, so iteration order no longer varies between runs.
- **Sleep:** a body at or above its threshold on linear speed alone skips the corner-spin term.
- **Body slots:** a body keeps a `Weak` to its world, so `body::with_sim` does one registry lookup
  instead of two.

Allocations in the example: 1016 a step before, 44 after (Rapier's manifolds).

**Tried, not kept: Rapier's `parallel` feature.** The staged solver gives bit-identical results at
1, 2 and 4 threads, but on `pile` it saved no CPU and almost no wall time (0.6 ms a step, mostly
serial), and the tick is measured in CPU.

## Results

Interleaved A/B (3 rounds each, same session, load ~30–37):

| Metric | Before | After |
| --- | --- | --- |
| Tick CPU median | 4.96 ms (4.95–5.11) | 3.90 ms (3.77–3.99) |
| Tick CPU p95 | 11.9 ms (11.4–13.5) | 13.0 ms (11.5–13.1) |
| Total CPU, 500 ticks | 2.85 s (2.80–2.86) | 2.32 s (2.27–2.39) |
| Main thread, 500 ticks | 2.18–2.34 s | 1.68–1.81 s |
| `perf stat`, 8 s window: instructions | 11.24 G | 7.77 G |
| cycles, task-clock | 10.76 G, 2.31 s | 8.86 G, 1.93 s |
| Physics alone, CPU per 500 steps | 614 ms | 300 ms |

`run.sh perf pile both` after (5 runs): ours 3.77 ms median (3.71–3.92), p95 12.7, total 2.30 s;
real 2.28 ms, p95 12.1, total 1.70 s (1.65×, was 1.98×). Main thread: ours 1.81 s, real 0.88 s
plus 0.24 s on its `GlobPool` workers.

p95 is the tiered JIT thread: the heaviest ticks bunch together early in the window, and that
thread's CPU varied 0.36–0.88 s between runs of the same build. Real's p95 moves just as much
(7.6–14.3). Read the median and total.

Behaviour is unchanged. The example's body and event hashes match the old build bit for bit, and
the probe, orbmatch and zoo markers of the old and new builds are identical at tolerance 0 (and
identical between two runs of the new build). Divergences from real stay at 324 (probe), 54
(orbmatch) and 32 (zoo).

## What's left

Ours spends about twice real's main-thread CPU. The solver (4 iterations of 2 PGS plus 3
stabilization, fitted in docs/differential.md) is now the biggest engine item, then managed code
and the CLR, then our wrapper (about 10%) and `objects::get` (about 6%).
