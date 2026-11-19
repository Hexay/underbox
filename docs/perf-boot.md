# Boot performance

Process start to the first scene frame (`PERF:START`), ours only: real's client also loads and ticks the
main menu, ours skips it, so ours-before vs ours-after is the comparison. Linked from
docs/differential-perf.md.

## Tools

- `tools/diff/boot-profile.sh <scenario> [label]`: one `run.sh perf` boot under `perf record`
  (task-clock, 999 Hz, `DOTNET_PerfMapEnabled=1`), report by boot phase, thread class, DSO and symbol.
  `ENGINE=real`, `BOOT_PROFILE_CALLGRAPH=1` optional. Out: `target/diff/boot-profile/<scenario>-<label>/`.
- `tools/r2r-pending.py <dir>`: managed dlls still IL-only, read from PE headers.

## Profile (2026-11-19, IL managed, warm caches)

Client `showroom`, 19.5 s wall, 49.4 s CPU:

| phase | wall s | CPU s | on the critical path |
|---|---|---|---|
| pre (start → WebSurface) | 2.8 | 4.7 | .NET startup, JIT |
| wait (Steam web-ticket poll) | 14.3 | 40.7 | the poll itself; TP workers (Roslyn compile, TypeLibrary) 30.5 s finish inside it |
| post (ticket → PERF:START) | 2.4 | 4.1 | main 2.0 s: managed scene load 1.0, engine 0.27 (naga WGSL parse, decl extract), lavapipe 0.14 |

CPU by DSO up to PERF:START: JIT'd managed 17.5, libcoreclr 12.8, libclrjit 6.4, kernel 5.2,
libengine2 1.8, libc 1.6, CoreLib 1.5. By managed assembly: Sandbox.Engine 10.7, CodeAnalysis.CSharp
10.4, Sandbox.Reflection 6.0, CodeAnalysis 5.5. The main thread's 9.8 s in the wait is
`SyncContext.RunBlocking` spinning (`Thread.Yield`), a fixed cost.

Server `pile`, 5.4 s wall, 10.7 s CPU: TP workers 8.9 (Roslyn), main 1.7; libclrjit 4.3, libcoreclr
3.9, libengine2 0.05. Wall is bounded by the pool's compile.

Engine2 is ≈2–3 s of client CPU and ≈0.4 s of its critical path; on the server, noise. Boot is managed
JIT and Roslyn.

## What changed

1. **ReadyToRun actually runs.** `tools/r2r-managed.sh` skipped every dll: its stamp was touched between
   the server and client builds, and msbuild copies dlls with their old mtimes, so 0 of 79 were R2R. It
   now picks pending dlls from PE headers (`tools/r2r-pending.py`), records ones crossgen2 rejects so
   they aren't retried, and logs why. 79 dlls take ~23 s at 4 jobs, once; a no-op run is 0.02 s.
2. **Shader translation cache key** (`crates/shaders/src/compiler.rs`, `keyed_source`): the key hashed
   the expanded source including `#line "<game copy>/core/shaders/…"`, so ~20 programs (stand-in,
   CMAA2, SPD, occlusion, skinning, cluster cull) missed whenever a boot got a new game copy (new
   worktree, parallel verify boot, sbox-public change). The key now uses each marker's file name only;
   entries written under two copy paths were byte-identical, and no shader uses `__FILE__`. No
   `PASSES_VERSION` bump: SPIR-V output is unchanged.

## Before / after

`UNDERBOX_ENGINE2=<so> run.sh perf <scenario> ours`, builds alternated run by run, 3 rounds, median
[min–max]. Before: base engine, IL managed. After: key fix, R2R managed.

| scenario | wall s before | wall s after | CPU s before | CPU s after |
|---|---|---|---|---|
| showroom client | 20.7 [17.9–26.6] | 20.2 [19.0–23.6] | 47.8 [42.9–48.4] | 37.0 [34.3–38.2] |
| skinned client | 19.5 [18.9–20.3] | 18.4 [16.9–18.6] | 48.6 [46.1–49.4] | 36.7 [36.5–37.7] |
| pile server | 5.24 [5.12–5.48] | 3.81 [3.68–4.11] | 10.6 [10.3–10.9] | 6.21 [5.90–6.47] |
| probe server | 6.69 [6.39–7.41] | 4.99 [4.49–5.02] | 13.2 [13.0–13.3] | 7.95 [7.72–8.12] |

Client wall barely moves: the Steam ticket poll (13–16 s) hides the managed work that R2R removes.
Whole-run CPU (boot + 60 warmup + window): showroom 106 → 100 s, skinned 99.5 → 91.8 s.

R2R shifts tier-1 rejits later: with `WARMUP_FRAMES=60` the window's tiered-JIT thread rises from ~1 s
to 6–8 s, and frame engine CPU reads ~35% higher (82.9 → 116 ms). With `WARMUP_FRAMES=300` it's
gone: engine 68.8 [62.7–74.9] vs 72.9 [71.2–74.6] ms, lavapipe 106 vs 112 ms (2 rounds, within run
spread). Frame metrics measured after this lands want ≥300 warmup frames, or they compare JIT timing.

## Warm vs cold caches

`showroom`, IL, base engine, 2 rounds. Cold-x: empty translation cache; cold-m: empty Mesa disk cache.

| | boot CPU s | warmup wall s | warmup engine CPU s | warmup lavapipe CPU s |
|---|---|---|---|---|
| warm | 46.6 | 8.8 [6.7–11.0] | 9.9 | 6.6 |
| cold-x | 46.9 | 10.4 [5.1–15.8] | 11.0 | 7.0 |
| cold-m | 46.1 | 12.3 [10.2–14.3] | 14.6 | 7.1 |
| cold both | 47.5 | 16.2 [14.9–17.5] | 15.5 | 7.3 |

Boot is unaffected: no shader compiles before PERF:START. Cold caches cost the first frames, up to
~7 s wall. The key fix turns the per-new-copy case from partly cold-x into warm.

## Output lock

- Frame hashes of showroom, outdoor, fog, skinned, glass, particles, decals: identical, base + IL vs
  key fix + R2R with an empty translation cache.
- `run.sh state` on `pile`: `ours.jsonl` byte-identical, base + IL vs key fix + R2R.

## Negative results

- Engine work during the ticket wait (`current_config`, `run_callbacks`) runs inside managed's spin;
  making it cheaper only spins more.
- Compiling shaders before PERF:START would add boot time: none are compiled before it today.
- R2R for Roslyn alone (CodeAnalysis*) gets most of the CPU (49 → 37 s), but all 79 is no worse.
- Server perf attach once wrote an empty `perf.data`; `boot-profile.sh` now runs it in its own
  session and keeps perf's stderr in `perf.err`.
