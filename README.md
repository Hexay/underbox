# underbox

underbox is an open-source Rust implementation of `engine2`, the closed native core under s&box.
Facepunch publishes s&box's C# engine as [sbox-public](https://github.com/Facepunch/sbox-public)
under MIT, but it only runs on top of their closed binaries. underbox replaces those binaries, so
that engine and the games built on it run unchanged with no closed engine code underneath.

Not affiliated with or endorsed by Facepunch Studios or Valve. s&box is a trademark of Facepunch
Studios. underbox ships no Facepunch or Valve content: you bring your own sbox-public checkout and
game content.

## Status

Pre-release. Games have only been run on Linux.

- Interop table: all 4,678 slots of the `igen_engine` table are bound to code (3,519 in
  subsystems, 937 field accessors, 222 casts, 0 stubs), per
  `cargo run -q -p engine2 --bin coverage`. Bound is not the same as verified: per-slot contract
  tests are still being written ([PLAN](docs/PLAN.md)).
- Real games ([real-games.md](docs/real-games.md)), against the sbox-public commit pinned in
  `abi/SBOX_PUBLIC_COMMIT`:
  - [Facepunch/sandbox](https://github.com/Facepunch/sandbox) runs on a dedicated server, a client
    joins it and spawns, the player can be moved in an X11 window, and sounds reach the mixer.
  - [apetavern/grubs](https://github.com/apetavern/grubs) starts and plays a free-for-all round.
  - sbox-public's own Sweeper sample and two project templates load, run and render
    ([sample-games.md](docs/sample-games.md)).
- Rendering: wgpu on Vulkan, with s&box's shaders translated from HLSL. Golden-frame tests cover
  lighting, shadows, fog, skinning and UI panels ([renderer.md](docs/renderer.md)).
- Steam: an offline backend by default; a Steam backend behind `--features steam`. The
  online paths still need manual checks with a logged-in account ([steam.md](docs/steam.md)).

What doesn't work yet:

- Game frames are untextured, and most models render as the error model. Public game repos ship
  sources rather than compiled assets, and compiling models is Facepunch's closed tooling.
- The game runs block package downloads from sbox.game, so models and assets that games take from
  packages are missing.
- No differential testing against the real engine2 until it's clear the s&box EULA allows it.
- Deliberately out of scope: ray tracing, MSAA, the Steam HTML surface, the editor and its tools.
  Open renderer work is under "Next" in [renderer.md](docs/renderer.md).

## Requirements

- Rust 1.99 or newer (`rust-version` in `Cargo.toml`).
- On Debian or Ubuntu, the packages CI installs:

  ```sh
  sudo apt-get install build-essential pkg-config cmake protobuf-compiler libprotobuf-dev libssl-dev \
    libx11-dev libxext-dev libxcursor-dev libxfixes-dev libxi-dev libxrandr-dev libxss-dev \
    libxtst-dev libudev-dev libdbus-1-dev mesa-vulkan-drivers libvulkan1
  ```

  `mesa-vulkan-drivers` provides lavapipe, a software Vulkan driver, so the render tests run
  without a GPU.
- To run games: the .NET 10 SDK and a checkout of sbox-public.
- For shader translation: `tools/fetch-dxc.sh` downloads Microsoft's DXC release, pinned by
  version and checksum.

Windows: the workspace builds and its unit tests pass in CI. The boot, game and release scripts
are POSIX shell and have only been run on Linux.

## Build

```sh
git clone --recurse-submodules https://github.com/Hexay/underbox
cd underbox
cargo build -p engine2
```

This builds `libengine2.so`, which takes the place of Facepunch's library in an sbox-public game
directory. `tools/dist.sh` builds a portable release copy ([release.md](docs/release.md)).

## Running a game

The scripts look for sbox-public next to this repo (`../sbox-public`) unless given a path
(`tools/sbox-path.sh`). They build its launcher, fill in what its build needs without Facepunch's
binary download (`tools/prepare-sbox.sh`), and install the fresh libengine2 into a copy of its game
directory. Their scratch state goes under `UNDERBOX_CACHE`, default `~/.cache/underbox`
([dev-loop.md](docs/dev-loop.md#scratch-directories)).

```sh
sh tools/match.sh                     # headless server plays a scripted match from corpus/
MODE=join sh tools/game.sh ../sandbox # a real game; modes and checks in docs/real-games.md
```

## Checks

```sh
sh tools/test.sh <crate> [filter] # one crate's tests
sh tools/verify.sh               # the full gate: tests, clippy, boots, golden frames, coverage
```

`verify.sh` must print `RESULT: PASS` before anything is merged. Every command is listed in
[dev-loop.md](docs/dev-loop.md).

## Layout

| Path | Contents |
| --- | --- |
| `crates/engine2` | the library: `igen_engine` and `CreateInterface` exports, glue per engine class |
| `crates/abi`, `abi-spec`, `abi/`, `tools/abigen`, `tools/interop-dump` | the slot table and struct layouts, generated from sbox-public (`tools/regen.sh`) |
| `crates/render`, `shaders`, `window` | wgpu renderer, HLSL to SPIR-V to WGSL translation, SDL3 windows and input |
| `crates/assets`, `kv`, `world`, `objects` | Source 2 resources, KeyValues3, VPK and maps |
| `crates/physics`, `box2d-sys`, `anim`, `audio` | Rapier and Box2D v3 physics, animation graphs, audio mixer |
| `crates/gns`, `platform` | GameNetworkingSockets, Steam and offline platform backends |
| `corpus/` | small test games and scenes with golden frame hashes |
| `third_party/` | GameNetworkingSockets, Box2D and protobuf as git submodules |
| `docs/`, `research/` | design notes and file-format research |

## Docs

- [PLAN.md](docs/PLAN.md): scope, architecture, milestones, open questions
- [dev-loop.md](docs/dev-loop.md): every script and how the boot scripts share state
- [real-games.md](docs/real-games.md), [sample-games.md](docs/sample-games.md): game runs and their evidence
- [renderer.md](docs/renderer.md), [lighting.md](docs/lighting.md), [fog.md](docs/fog.md), [skinning.md](docs/skinning.md)
- [audio.md](docs/audio.md), [play-and-sound.md](docs/play-and-sound.md), [steam.md](docs/steam.md), [release.md](docs/release.md)

## Contributing

Issues and pull requests are welcome. Run `sh tools/verify.sh` before opening a PR. Generated code
under `crates/abi` comes from `tools/regen.sh` and is never edited by hand. Test content must be
synthetic (see the fixture writers in `crates/assets/src/compiled`); don't commit anything taken
from Facepunch, Valve or Steam.

## Licence

MIT, see [LICENSE](LICENSE). Files derived from sbox-public and the third-party components under
`third_party/` keep their own licences; see [NOTICE](NOTICE).
