# Dev loop

| Step | Command |
| --- | --- |
| Unit tests | `tools/test.sh <crate> [test-filter]`: failures and totals only, exits with cargo's status |
| Boot the server | `flock -o /srv/agents/cache/underbox-boot.lock sh tools/boot-server.sh [max-s] [sbox-public]` |
| Scripted match | `tools/match.sh` (takes the lock, stops at `MATCH:END`) |
| Net probe | `tools/net-probe.sh` (takes the lock) |
| Measure a loop | `tools/loop-bench.sh <runs> <file> <cmd...>` (median and spread) |

## Rules

- Don't pipe `cargo test` into `grep`/`tail`: the pipe hides a failing exit. Use `tools/test.sh`.
- Boots share `sbox-public/game/bin`: always under the boot lock, and with `flock -o`, so a lingering
  child (an MSBuild node, a server) can't inherit the lock and stall every other worktree.
- `boot-server.sh` stops `GRACE` seconds after a log line matches `UNTIL`; a plain boot stops at
  `Server is idle`. Give game boots an `UNTIL` that marks "done" instead of idling to the timeout.
  The last line, `== timing:`, splits the boot into prepare, cargo and server.
- `boot-server.sh` ReadyToRun-compiles `game/bin/managed` (`tools/r2r-managed.sh`, a no-op once
  done) so boots don't JIT the engine and Roslyn. A dotnet rebuild there is recompiled next boot.

## Incremental

The agent environment sets `CARGO_INCREMENTAL=0`, which overrides the dev profile and turns off
incremental compilation for workspace crates. sccache doesn't need that: it passes incremental
(workspace) compiles straight through and still caches registry crates. The `tools/*` scripts unset
the variable, which roughly halves an abi edit → engine2 test cycle.

Switching between a plain `cargo` (variable set) and a `tools/*` script rebuilds the workspace
crates once (about 5 s). Removing `CARGO_INCREMENTAL=0` from the agent environment ends that.
