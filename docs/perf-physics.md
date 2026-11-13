# Server tick profile (pile)

Part of docs/differential-perf.md. Scenario `pile` (328 dynamic bodies inside a keyframed floor and
walls, headless dedicated server, `taskset 0-7`), 2026-11-12 and 13, shared box at load 15–60.

## Tools

| What | How |
| --- | --- |
| Tick CPU, ours vs real | `tools/diff/run.sh perf pile both` |
| A/B of two builds | `UNDERBOX_ENGINE2=<so> tools/diff/run.sh perf pile ours`, alternating the builds run by run |
| Profile the server | `perf record -F 1999 -p <dotnet pid>` during the ticks, `DOTNET_PerfMapEnabled=1` for managed frames; `perf stat -p` for counters |
| A/B that survives load | `perf record -e instructions:u -c 500000 -p <pid>`: instruction samples bucketed by area, the pacer spin left out. CPU time of one build moved 0.75–1.64 s between runs on the shared box; instructions per area moved under 1% |
| Who runs on which thread | `perf record --switch-events` (plus `-s` for per-thread counts) |
| Physics alone | `cargo run --release -p physics --example pile [ticks=750] [warmup=250] [keyframed\|static]`: the pile in a bare `World`, per-step time, CPU of all threads, a hash of every body's final state and of every contact event |
| Allocations | `heaptrack` on the example at two tick counts; the difference is per tick |

`perf_event_paranoid=1` is enough for all of these, no root needed. Release `libengine2.so` keeps
its symbol table, but `--call-graph dwarf` doesn't unwind through it: use flat profiles.

## Round 1

Ranking before, as a share of the server's samples during the ticks: frame pacer spin 19.7%,
Rapier solver 16.1%, managed code 15.3%, tiered JIT 14.3%, our contact/persist/bounce/sleep code
8.8%, SipHash 6.9%, CLR 5.4%, touch re-measure 3.8%, interop 3.4%.

Changes:

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
- **Body slots:** a body keeps a `Weak` to its world, so a slot does one registry lookup, not two.

Allocations in the example: 1016 a step before, 44 after (Rapier's manifolds). Tick CPU median
4.96 → 3.90 ms, `perf stat` instructions 11.24 G → 7.77 G over an 8 s window.

## Round 2: ranking

Instruction samples of the server during the ticks (all threads, pacer spin left out), about
5.7 G a window:

| Area | Share | Per tick |
| --- | --- | --- |
| Rapier (solver, narrow phase, islands) | 42% (77% of instructions) | ~1.5 ms |
| Managed JIT code (same on both engines) | 24% | ~0.88 ms |
| Interop: slots, thunks, `objects`, `OnActive` | 10% | ~0.36 ms |
| Pacer spin (`SDL_DelayPrecise`, vdso clock) | 7% | |
| Our wrapper (contacts, persist, bounce, sleep) | 7% | |
| CLR runtime and tiered JIT | 7% | |

Shares are of cycles; Rapier's instruction share is higher because its SIMD code runs at a high IPC.

The tap (`UNDERBOX_TAP`) shows the same managed calls per tick on both engines: 328 each of
`IsSleeping`, `GetTransform`, `GetLinearVelocity`, `GetAngularVelocity` and `OnActive`. Real adds 5
`SetTargetTransform` and 5 `OnActive` calls for the keyframed bodies. Interop is not where the gap is.

**Why physics in the server costs 3× the old example.** The scene's floor and walls are keyframed:
a managed `Collider` on a non-static GameObject makes a kinematic body. Rapier's solver colouring
(`assign_pair_solver_color` in `narrow_phase`) exempts only fixed bodies, so every contact with the
floor conflicts with every other contact with it, overflows the coloured band and lands in the
serial tail. The example now builds keyframed walls by default: 1.31 ms a step, against 0.56 ms
for `static`. Body and event hashes for the keyframed pile: `396ed9c49ab98b5d` / `826863f0dc3de010`.

## Round 2: real's GlobPool workers

From `perf record --switch-events` on real:

- 7 `GlobPool` threads use 0.5–0.64 ms a tick in all, about 45% of it in the kernel (wakeups).
- Their user time is in the same `libengine2` range as the main thread's own physics time (8–9%
  of main). It's a fork-join: the main thread hands out the step's jobs and works on them too.
- Every interop call the tap sees happens on the main thread; no callbacks run on workers.
- Real's physics is about 0.83 ms of CPU a tick, 0.38 ms of it on main. Its managed cost (about
  0.86 ms) and its pacer spin match ours.

So real is about half our physics CPU, and it also spreads that work across threads. The managed
side is a wash.

## Round 2: changes

- **Getters don't mark bodies modified.** `RigidBodySet::get_mut` queues the body as user-changed,
  so every step Rapier re-runs `handle_user_changes_to_rigid_bodies` on it and re-syncs its island.
  Read-only slots (transform, velocities, damping, type, sleep and enabled state, aggregate origin
  and mass centre) go through `body::read_rb`, which takes `&RigidBody`.
- **Sleep arming skips no-ops.** `arm_sleep` leaves a body alone when its activation thresholds
  already say what it would write (`activation_mut` sets `RigidBodyChanges::SLEEP`).
- **One registry pass per active body.** `objects::with` runs a closure under the registry's read
  lock instead of cloning the `Arc`; `OnActive` gathering and `body::sim_of` use it.

## Round 2: tried, not kept

- **Rapier `parallel` on the keyframed pile:** bit-identical at 1, 2 and 4 threads, no wall-time
  gain, 587–958 ms of CPU against 512–538 ms serial. The serial tail from the kinematic colouring
  leaves nothing to spread.
- **`codegen-units = 1` for `rapier3d` and `parry3d`:** no change.
- **Mod-tracking avoidance alone in the example:** 8.85 G → 8.76 G instructions (1%).

## Results

Interleaved `run.sh perf pile both`, 3 rounds per build, load 18–25:

| Metric | Before | After |
| --- | --- | --- |
| Tick CPU median (median of runs) | 3.65 ms (3.51–3.72) | 3.53 ms (3.36–3.57) |
| Total CPU, 500 ticks | 2.20 s (2.15–2.22) | 2.13 s (2.07–2.18) |
| Main thread | 1.72 s (1.67–1.76) | 1.67 s (1.62–1.68) |
| Ratio to real | 1.65× (1.64–1.75) | 1.63× (1.56–1.65) |

Real in the same runs: 2.09–2.26 ms median, main 0.72–0.86 s plus 0.22–0.34 s on `GlobPool`.
Instruction A/B (3 rounds each): Rapier 4.39 → 4.38 G, interop 0.18 → 0.18 G, wrapper 0.36 →
0.36 G. The CLR bucket moved 0.29–0.85 G from run to run of the same build (tiered JIT). The gain
is about 3% and sits at the noise floor.

p95 is the tiered JIT thread and swings 7–15 ms on both engines. Read the median and total.

Behaviour is unchanged. Example hashes match the old build for static and keyframed walls. Probe,
orbmatch and zoo markers of the old and new builds are identical at tolerance 0 (1789, 132 and 45
markers). Divergences from real stay at 324 (probe), 54 (orbmatch) and 32 (zoo).

## Round 3: kinematic bodies colour like fixed ones

Rapier is patched (third_party/rapier3d, `[patch.crates-io]`; the patch, why it's sound and what
was left alone: third_party/rapier3d/UNDERBOX.md). The colouring treats a kinematic body as it
treats a fixed one, so contacts with the keyframed floor take only the dynamic body's colour mask.
On the headless keyframed pile, 186 of about 427 contacts a step used to overflow into 1-lane
serial chunks (334 chunks); now none overflow and the layout is the static pile's (about 111
chunks). Floor contacts now sit in the top colour band, solved last each sweep, as fixed
geometry's already were.

Headless pile, interleaved, 3 rounds, load ~25:

| Walls | Before: step median, CPU per 500 steps | After |
| --- | --- | --- |
| keyframed | 1.34 ms, 549–552 ms | 0.75–0.77 ms, 327–338 ms |
| static | 0.55–0.58 ms, 279–299 ms | 0.54–0.60 ms, 270–293 ms |

Instructions for a whole 750-step run: keyframed 8.64 G → 5.34 G, static 4.70 G on both builds.
Keyframed is still about 0.2 ms behind static; its solver layout now matches static's, and its
trajectory differs (other contacts), so the rest is elsewhere (not profiled further).

Interleaved `run.sh perf pile both`, `REPEATS=1` × 3 rounds per build, load 25–32, instructions from
`perf stat -p` between the first `PERF:TICK` and `PERF:DONE`:

| Metric | Before | After |
| --- | --- | --- |
| Tick CPU median | 3.52–3.72 ms | 2.99–3.05 ms |
| Total CPU, 500 ticks | 2.06–2.22 s | 1.92–1.98 s |
| Main thread | 1.64–1.77 s | 1.45–1.49 s |
| Instructions | 8.86–9.50 G | 6.96–7.29 G |
| Ratio to real, median | 1.34–1.50× | 1.15–1.21× |

Real in the same runs: 2.34–2.77 ms median, 1.69–1.98 s total, main 0.86–1.02 s, 4.90–5.61 G
instructions. Real ran slower than in round 2 (2.09–2.26 ms), so compare ratios within a round.

Behaviour: deterministic (two runs of each build identical at tolerance 0 on probe, orbmatch and
zoo; example hashes repeat). Static-wall example hashes are unchanged; keyframed ones are now
`57a4a5e26da5184e` / `5a3c86598a03dbd0`. Probe (1789 markers) and orbmatch (132) markers are
identical to the unpatched build's, so 324 and 54 divergences. The zoo moved: unpatched (with
round 2's changes) it logs 50 markers and 33 divergences, its trike still awake at the first
`GAME:TICK`s (awake 9, 8, 4 against real's 6, 5, 5) and four `GAME:AWAKE` lines real doesn't log;
patched, 45 markers and 32 divergences (awake 8, 5, 0), `GAME:RESTED` unchanged. `tools/test.sh
physics` passes unchanged (landing, stack, car fits included), so no iteration refit.

## What's left

Our main thread is still about 1.6× real's, and real spreads its physics over `GlobPool` workers.
Rapier's `parallel` feature was dropped while the serial tail held the step (round 2); with the
tail gone it is worth measuring again for wall time, though the tick is measured in CPU. The
keyframed pile's remaining 0.2 ms over static is unexplained.
