# underbox

An open Rust replacement for s&box's closed native core (`engine2`). The MIT C# engine
(Facepunch/sbox-public) and games built on it run on it unchanged: dedicated servers, and clients
that render through wgpu. Every slot in the `igen_engine` interop table is bound
(`cargo run -p engine2 --bin coverage`).

Not affiliated with Facepunch. Ships no Facepunch or Steam content.

## Layout

| Path | What |
| --- | --- |
| `crates/engine2` | the cdylib: `igen_engine`/`CreateInterface` exports, slot glue per engine class |
| `crates/abi`, `abi-spec`, `tools/abigen`, `tools/interop-dump` | generated slot table and layouts (`tools/regen.sh`) |
| `crates/render`, `shaders`, `window` | wgpu renderer, HLSL→SPIR-V→WGSL translation, SDL3 |
| `crates/assets`, `kv`, `world` | Source 2 resources, KeyValues3, VPK and maps |
| `crates/physics`, `box2d-sys`, `anim`, `audio` | Rapier + Box2D v3, animation graphs, mixer |
| `crates/gns`, `platform` | GameNetworkingSockets, Steam/offline platform backends |
| `corpus/` | test games and scenes with golden frame hashes |
| `docs/`, `research/` | design and format notes; start at `docs/PLAN.md` |

## Build and check

Needs Rust (see `rust-version`), a local sbox-public checkout (`tools/sbox-path.sh`), the .NET SDK
and Vulkan (llvmpipe works headless).

```sh
sh tools/verify.sh        # merge gate: tests, clippy, boots, golden frames, coverage
sh tools/match.sh         # headless server plays a scripted match
sh tools/client-frame.sh  # headless client frame vs golden
sh tools/dist.sh          # portable release build (docs/release.md)
```

Full command list: `docs/dev-loop.md`.

## Status

- Server and client boot and run the corpus on Linux; Windows builds and tests in CI.
- Differential runs against real engine2 have started; divergences are in `docs/differential.md`.
  Steam online features need the manual checks in `docs/steam.md`.
- Deliberately unavailable: ray tracing, MSAA, the Steam HTML surface, editor build jobs.
  Open renderer work: `docs/renderer.md` "Next".

## Licence

MIT. Third-party code under `third_party/` keeps its own licence; see `NOTICE`.
