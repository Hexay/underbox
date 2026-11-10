# Dev loop

| Step | Command |
| --- | --- |
| Unit tests | `tools/test.sh <crate> [test-filter]`: failures and totals only, exits with cargo's status |
| Boot the server | `tools/boot-server.sh [max-s] [sbox-public]` |
| Scripted match | `tools/match.sh` (stops at `MATCH:END`) |
| Net probe | `tools/net-probe.sh` |
| Shared-build lock | `tools/boot-lock.sh <cmd...>` (only for scripts that write `sbox-public/game`) |
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

`sbox-public/game` holds the shared managed build: `game-dir.sh` builds sbox-server there and
ReadyToRun-compiles `game/bin/managed` (`tools/r2r-managed.sh`), both under the boot lock. Each
worktree then boots in its own reflinked copy, `/srv/agents/cache/underbox-game/<worktree>-<hash>`,
re-copied (about 0.5 s, no extra disk; `logs/` is skipped) when a managed dll or `.underbox-*`
stamp in the shared dir is newer than the copy. With nothing to do, no lock is taken. A copy can be
deleted any time. Ports come from `tools/worktree-port.sh`, so servers don't collide either.

A script that changes the shared build (a dotnet build into `game/`) runs under `boot-lock.sh`;
copies pick the change up on their next boot.

## Incremental

The agent environment sets `CARGO_INCREMENTAL=0`, which overrides the dev profile and turns off
incremental compilation for workspace crates. sccache doesn't need that: it passes incremental
(workspace) compiles straight through and still caches registry crates. The `tools/*` scripts unset
the variable, which roughly halves an abi edit → engine2 test cycle.

Switching between a plain `cargo` (variable set) and a `tools/*` script rebuilds the workspace
crates once (about 5 s). Removing `CARGO_INCREMENTAL=0` from the agent environment ends that.

## CI

Windows builds GameNetworkingSockets against the cached `VCPKG_ROOT` protobuf in classic mode;
manifest mode (GNS ships a `vcpkg.json`) rebuilt protobuf, openssl and abseil on every cold run
(about 55 min). Windows runs tests only; Linux runs clippy for both. rust-cache saves on failure too.
