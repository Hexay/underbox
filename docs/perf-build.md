# Build-level CPU: allocator and release profile

Part of docs/differential-perf.md. Measured 2026-11-18 on main `dee0cc7`, load 23–100 (shared box).

## What changed

- **mimalloc is libengine2's global allocator** (`crates/engine2/src/lib.rs`). Feature
  `local_dynamic_tls`: libengine2 is dlopened by dotnet, and mimalloc's default initial-exec TLS
  must fit glibc's small static-TLS surplus. No `override`: only Rust allocations move; C code
  (SDL, GNS, protobuf, lavapipe) and dotnet keep libc's malloc.
- **FFI ownership audit:** nothing crosses the boundary to be freed by the other side. Managed's
  `PassBackString`/`NativeMemory.Alloc` buffers are freed by managed; `window::free` only frees
  SDL's memory; Steam P2P messages are boxed and released by our own callback
  (`net/p2p/message.rs`). A new path that hands Rust memory to C/managed `free` (or the reverse)
  would break.
- **No `[profile.release]` change:** no LTO or codegen-units setting paid for itself (below).

## Method

`tools/diff/run.sh perf <scenario> ours` with `UNDERBOX_ENGINE2=<so>`, builds round-robin, one run
each per round. Thread CPU swung ±20% with load, so each run also recorded `perf record -e
instructions:u` on the dotnet process (clock `mono`), cut to the marker window and bucketed by
DSO/symbol (libengine2, mimalloc's `mi_*`, libc's malloc family, lavapipe's threads left out).
Instruction counts per frame repeated within 1–2%.

## Allocator (healthy frames)

3 rounds, system allocator vs mimalloc, both with the per-frame resource sweep disabled locally
(see the regression below). Medians [min–max].

| Metric | skinned system | skinned mimalloc | showroom system | showroom mimalloc |
| --- | --- | --- | --- | --- |
| Main thread ms | 4.36 [4.24–5.85] | 4.14 [4.05–5.51] | 4.46 [3.21–4.48] | 4.38 [4.24–4.56] |
| Main thread M instr/frame | 17.48 | 16.23 (−7.2%) | 11.03 | 10.05 (−8.9%) |
| libc malloc M instr/frame, engine threads | 25.2 | 20.4 | 23.9 | 20.9 |
| Engine CPU ms | 69.6 [64.8–88.3] | 72.1 [64–83.7] | 78.2 [62.9–78.2] | 79.9 [76.9–82.8] |
| Peak RSS MiB | 1264 | 1299 | 1217 | 1242 |

Pile (5 rounds): tick CPU median 2.76 [1.92–3.12] → 2.80 [1.83–2.87] ms, libengine2 7.47 → 7.54
M instr/tick, peak RSS 474 → 503 MiB. The tick allocates almost nothing, so no change is expected.
Boot CPU is unchanged on all three. RSS grows 2–6% (mimalloc keeps freed segments).

## Release profile

Warm = touch `crates/engine2/src/lib.rs`, then `cargo build --release -p engine2 -j4` (what
`perf.sh` pays per edit). Cold times depend on sccache hits. Pile example: `cargo run --release
-p physics --example pile`, 9 rounds, total `instructions:u`; every variant gave the same state
and event hashes. Pile engine2: M instr/tick in the server pile, with mimalloc, 5 rounds.

| Profile | Cold s | Warm s | `.so` MB | Pile example instr | Pile engine2 M instr/tick |
| --- | --- | --- | --- | --- | --- |
| default (cgu 16, thin-local) | 271 | 27 | 47.1 | 1.000 | 7.54 |
| `lto = "thin"` | 219 | 46 | 47.1 | 0.985 | 7.53 |
| `codegen-units = 1` | 180 | 65 | 41.0 | 0.994 | — |
| thin + cgu 1 | 305 | 296 | 41.0 | 0.981 | — |
| `lto = "fat"` + cgu 1 | 447 | 509 | 41.0 | 0.981 | 7.40 |

Pile tick CPU stayed within noise for all (2.74–2.81 ms medians). On skinned, fat LTO moved
libengine2's instructions by −0.3%. Fat LTO's best case (−1–2%) isn't worth a 19× slower perf
loop, and a dist-only profile would ship code that `perf.sh` never measured. Rejected.

## Rejected without measuring

- **`panic = "abort"`:** `shaders::translate::spirv_to_wgsl` catches naga's panics at runtime
  (DXC output reaches its `unreachable!`s); abort would kill the client on such a shader.
- **`target-cpu`:** docs/release.md promises any x86-64 machine with glibc 2.35+; it states no
  CPU floor, and nothing here was SIMD-bound enough to justify adding one.
- **jemalloc:** not tried; it would need `disable_initial_exec_tls` for the same reason.

## Regression found: shaders rebuilt every frame

On `dee0cc7`, skinned's main thread costs 230–290 ms/frame (2.4–2.8 G instr) and showroom's
99–189 ms, against ~4.4 ms with the sweep off. The profile is naga's WGSL front end
(`Typifier::grow`, the lexer), `shaders::decl::extract` and lavapipe's NIR passes.
`bdfb281` ("unload unreferenced resources each engine frame", `app/platform.rs`) sweeps every
`Vfx` that only `shader::LOADED` holds. The next draw's `shader::find` parses it again, and the
new `Arc<VfxShader>` fails `render::shading::Generation`'s `ptr_eq`, so the program is rebuilt.
A missing shader also re-logs `shaders/complex.shader: not found` (~260 times per run). The
docs/differential-perf.md sweep predates this commit.

Fixed in `8921040`: `shader::find` loads through `ByName::find_or_insert_resident`, which the
sweep skips. Skinned main thread 5.36 ms [4.69–6.03] after, 2 runs at load ~28.
