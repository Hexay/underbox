# Dev loop

| Step | Command |
| --- | --- |
| Unit tests | `tools/test.sh <crate> [test-filter]`: failures and totals only, exits with cargo's status |
| Boot the server | `tools/boot-server.sh [max-s] [sbox-public]` |
| Boot the client | `tools/boot-client.sh [max-s] [sbox-public] [-- args]` (headless, logs frame hashes to `target/client/frames.tsv`) |
| Golden client frame | `tools/client-frame.sh` (showroom; `UNDERBOX_BLESS=1` records; PNG in `target/client-frame/`) |
| Scripted match | `tools/match.sh` (stops at `MATCH:END`) |
| Map loading | `tools/map.sh` (corpus/mapworld, stops at `MAP:END`); its frame: `CORPUS=mapworld tools/client-frame.sh` |
| Net probe | `tools/net-probe.sh` |
| Shared-build lock | `tools/boot-lock.sh <cmd...>` (only for scripts that write `sbox-public/game`) |
| Release build | `tools/dist.sh` (portable libengine2 in `dist/`; see docs/release.md) |
| Measure a loop | `tools/loop-bench.sh <runs> <file> <cmd...>` (median and spread) |

## Rules

- Don't pipe `cargo test` into `grep`/`tail`: the pipe hides a failing exit. Use `tools/test.sh`.
- Don't wrap the boot scripts in `flock`: they lock only what needs it, and run in parallel across
  worktrees. `boot-lock.sh` is re-entrant, waits at most `BOOT_LOCK_WAIT` (600) s, then prints the
  holder's pid and command line and exits 75.
- `boot-server.sh` stops `GRACE` seconds after a log line matches `UNTIL`; a plain boot stops at
  `Server is idle`. Give game boots an `UNTIL` that marks "done" instead of idling to the timeout.
  The last line, `== timing:`, splits the boot into prepare, cargo and server.
- `test.sh` and `boot-server.sh` check out missing submodules (`tools/submodules.sh`).

## Game dirs

`sbox-public/game` holds the shared managed build: `game-dir.sh` builds the server or client launcher there and
ReadyToRun-compiles `game/bin/managed` (`tools/r2r-managed.sh`), both under the boot lock. Each
boot then runs in a reflinked copy, `$UNDERBOX_GAME_DIRS/<worktree>-<hash>.vXXXXXX`
(about 0.5 s, no extra disk; `logs/` is skipped); `<worktree>-<hash>.current` names the newest. A
boot reuses that copy unless a managed dll or `.underbox-*` stamp in the shared dir is newer, or
another live boot uses it; then it gets a fresh one. Only shared work (a build, R2R, a copy from
the shared dir) takes the boot lock; the per-worktree `.lock` is held for well under a second.

A copy in use is never changed or deleted. A boot leases its copy (`.underbox-lease.<pid>`) before
building engine2, and a copy is deleted only once no live lease and no process cwd is in it. A
running server keeps loading by path (`libvfx_vulkan.so`, `getcwd`): re-copying under it used to
load a second libengine2, which aborts in protobuf ("File already exists in database"), or fail
in `GetCwd`. Unused copies can be deleted any time.

A script that changes the shared build (a dotnet build into `game/`) runs under `boot-lock.sh`;
boots pick the change up on their next start. Each boot borrows its port from
`tools/worktree-port.sh` (12 per worktree, held in `$UNDERBOX_PORT_HOLDS` while the
boot's script lives), so `match.sh`, `map.sh` and `net-probe.sh` run side by side, here and in
other worktrees. `boot-server.sh` borrows one unless the args carry `+port`.

`install-engine2.sh` installs NuGet natives via `nuget-file.sh` (cached in
`~/.cache/underbox/nuget`, written by rename), links them into `bin/managed` for .NET's fallback
probe (s&box's DLLImportResolver can miss an assembly), and exits 3 naming any required native
that is missing or not ELF. `match.sh` prints that line instead of an opaque DllNotFound.

## Scratch directories

The scripts keep shared state outside the checkout, under one root: `UNDERBOX_CACHE`, default
`${XDG_CACHE_HOME:-$HOME/.cache}/underbox`. Put it on btrfs or XFS for cheap reflinks. Each
directory below defaults to a path under it and can be set on its own; `tools/cache-dir.sh` derives
them, and every script sources it.

| Variable | Default under `$UNDERBOX_CACHE` | Holds |
| --- | --- | --- |
| `UNDERBOX_GAME_DIRS` | `underbox-game` | per-boot copies of `sbox-public/game` |
| `UNDERBOX_GAME_COPIES` | `underbox-real-games` | `tools/game.sh` copies of game checkouts |
| `UNDERBOX_PORT_HOLDS` | `underbox-ports` | port leases (`worktree-port.sh`) |
| `UNDERBOX_BOOT_LOCK` | `underbox-boot.lock` | the shared-build lock file |
| `UNDERBOX_DXC_DIR` | `dxc` | Microsoft's DXC release (`fetch-dxc.sh`; `crates/shaders` mirrors this default) |

All sessions that share an sbox-public checkout must share `UNDERBOX_BOOT_LOCK`.

Tests that read sbox-public take its path from `SBOX_PUBLIC`.

## Incremental

If your environment sets `CARGO_INCREMENTAL=0`, it overrides the dev profile and turns off
incremental compilation for workspace crates. sccache doesn't need that: it passes incremental
(workspace) compiles straight through and still caches registry crates. The `tools/*` scripts unset
the variable, which roughly halves an abi edit → engine2 test cycle.

Switching between a plain `cargo` (variable set) and a `tools/*` script rebuilds the workspace
crates once (about 5 s). Unsetting `CARGO_INCREMENTAL` in your environment ends that.

## CI

`.github/workflows/ci.yml` runs four parallel jobs: `clippy` and `test-linux` (Linux), `windows`
(tests only; Linux lints for both) and `dist` (an `ubuntu:22.04` container, docs/release.md).
Docs-only pushes (`*.md`, `docs/`, `research/`) skip CI. Shared steps are composite actions in
`.github/actions/` (`rust`, `linux-deps`, `dxc`).

- Tests need DXC: the `dxc` action runs `tools/fetch-dxc.sh` into a cached `UNDERBOX_DXC_DIR`.
- sccache caches Rust (`RUSTC_WRAPPER`, except clippy) and the cmake builds (GNS, SDL, protobuf);
  Windows uses Ninja because the Visual Studio generators ignore the compiler launcher.
- rust-cache saves only on main and keys on `Cargo.toml` too: it never re-saves a fully matched key,
  and its own key ignores `[profile]` sections.
- Windows builds GameNetworkingSockets against the cached `VCPKG_ROOT` protobuf in classic mode;
  manifest mode (GNS ships a `vcpkg.json`) rebuilt protobuf, openssl and abseil (about 55 min).
- apt's `.debs` are cached per runner image; a warm cache installs with `dpkg -i` and never touches
  the Ubuntu mirror, falling back to `apt-get` if that fails.
- Checkouts aren't recursive: GNS's nested submodules (webrtc, abseil) are only for ICE.
- Public repo: `contents: read` token, actions pinned by SHA, no secrets, no `pull_request_target`.
