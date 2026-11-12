# Performance differential against real engine2

Part of docs/differential.md (its rules apply). `tools/diff/run.sh perf <scenario> [both|ours|real]`
boots the same managed build and the same scene on our engine2 and on Facepunch's, several times,
and reports CPU cost, boot time and memory per engine.

| Scenario | Side | What |
| --- | --- | --- |
| `pile` | server | `tools/diff/pile/`: four 10-box stacks and 288 boxes and spheres dropped into a walled pit, all kicked upward every 100 ticks so nothing sleeps (328 bodies) |
| `probe` | server | the differential probe (docs/differential.md) |
| `showroom`, `skinned` | client | the render corpora (docs/differential-render.md) |
| any corpus or game checkout | `SIDE=server\|client` | as `run.sh state` |

Out: `target/diff/perf/<name>[-client]/report.txt`, plus each run's `<engine>-<i>/` with the boot
logs, `sample.json` and `load.txt`.

## How it measures

- **Markers.** `perf.sh` injects `tools/diff/PerfMark.cs` into a scratch copy of the scene
  (`tools/game/prepare.py`, as `state.sh`). It logs its own wall-clock timestamp at scene start
  (`PERF:START`), at every fixed tick after `WARMUP_TICKS` (server), or at the start and end of
  `FRAMES` frames after `WARMUP_FRAMES` (client). Both engines run the same marker code.
- **CPU.** `tools/diff/perf_sample.py` finds the `dotnet` process writing the boot's stdout and
  reads every thread's `/proc/<pid>/task/<tid>/schedstat` (nanoseconds of CPU, not wall time) about
  every 1.5 ms, plus `VmRSS`/`VmHWM` every 20 samples. A thread that exits keeps its last reading.
  CPU at a marker is interpolated at the marker's own timestamp, so log buffering doesn't move it;
  the report prints the median lag between a marker's timestamp and when the sampler saw it.
- **Server tick:** CPU between consecutive `PERF:TICK` markers, all threads: median, p95 and total
  over `TICKS` (500) ticks. The default 250-tick warmup lets .NET's tiered JIT settle; its thread
  still shows in "busiest threads".
- **Boot:** wall time and CPU from the process's start (`/proc/<pid>/stat` starttime) to
  `PERF:START`, the first frame the scene runs. Our cargo build and real's `assetc` pass happen
  before the process starts, so neither counts.
- **Memory:** peak is `VmHWM` at the end; steady is the median `VmRSS` inside the measured window.
- **Client frame:** engine CPU per frame is every thread except lavapipe's, whose names match
  `^(llvmpipe|lp:|lvp|gallivm)` or Mesa's shader-cache thread (`<process>:disk$N`)
  (`perf_sample.py --gpu`); lavapipe's CPU per frame is reported
  apart as the GPU-work proxy. Check the "busiest threads" line when a new Mesa renames them.
- **Fairness.** Each run is pinned with `taskset -c $CPUS` (default `0-7`) and `LP_NUM_THREADS`
  (default 4) on both engines, and both use the same lavapipe ICD (`tools/diff/lavapipe.sh`'s build
  when present, as `real-env.sh`). Ours runs a release build (`cargo build --release`, or
  `UNDERBOX_ENGINE2=<so>`), its frame capture off (`FRAMES=` to `boot-client.sh`), and is seeded with
  real's compiled core content as in `frame.sh`. Real's client gets a 1024×768 Xvfb, ours renders
  1024×768 offscreen; Xvfb's own CPU is not counted. Audio drains in real time on both: ours
  through engine2's null device (`UNDERBOX_AUDIO=null`), real through SDL's `dummy` driver
  (`PERF_SDL_AUDIO`). Under `boot-client.sh`'s ALSA null PCM, real's `SDLAudioP` thread spun
  for a whole core (55 s of CPU in a 200-frame window).
- **Client boot differs in work done:** both start the menu before `+game`, but ours logs
  "Couldn't find scenes/menu-main.scene" and skips it, while real loads and ticks the menu UI
  (its sbox.game calls fail fast through `noweb.sh`). Read client boot numbers with that in mind.
- **Repeats.** `REPEATS` (5) rounds, each ours then real, so slow drift in machine load hits both.
  The report gives each metric's median and min–max across runs and the ours/real ratio, and
  prints the load average at the start and end, flagged `NOISY` if the 1-minute load moved more
  than 25%.

## Reading the numbers

On this machine (no GPU, lavapipe on the CPU, other sessions loading it), the client numbers
measure **CPU cost, not GPU frame time**: "frame wall ms" depends on how busy the box is, and the
lavapipe column is how much rasterising the frame cost on the CPU, a proxy for GPU work. Thread
CPU time is much less sensitive to load than wall time, but cache and SMT contention still move it,
so compare runs taken together, and treat a `NOISY` report or a wide spread with suspicion.

With a real GPU, run the same command with no lavapipe ICD: `GPU=1 tools/diff/run.sh perf
showroom` leaves `VK_ICD_FILENAMES` alone, so both engines pick the hardware driver (real's
`-vulkan_allow_cpu` is then unused). Use `LP_NUM_THREADS` only with lavapipe. On a GPU,
"frame wall ms" becomes meaningful and the lavapipe column should read zero.

Server tick profile on `pile`, what was changed and before/after numbers: docs/perf-physics.md.

## Caveats

- `PERF:TICK` logs a line per tick on both engines; its cost is in both numbers.
- The sampler runs outside the `taskset` set but competes for the machine like everything else.
  At a 1.5 ms period, a single tick's CPU is uncertain by roughly the CPU burned in one period;
  medians and totals are much tighter than p95.
- Real's server also runs Steam's IPC threads (`IPC:CSteamEngin`); they are counted, as they are
  part of what real engine2 does each tick.
