# rapier3d 0.36.1, patched

The crates.io package of rapier3d 0.36.1 (Dimforge, Apache-2.0, `LICENSE`; upstream commit in
`.cargo_vcs_info.json`), used through `[patch.crates-io]` in the workspace `Cargo.toml`. Every
file is as published except the lines marked `underbox patch`. `LICENSE` comes from the upstream
repository at that commit, since the package doesn't ship one.

## The patch: kinematic bodies color like fixed ones

`src/geometry/narrow_phase/mod.rs` `color_body_is_world`, used by the two places that build a
pair's coloring descriptor (`contacts.rs`, the deferred coloring pass, and `pair_management.rs`,
recoloring after a body type or parent change). Upstream passes `rb.is_fixed()`, so a contact
with a kinematic body takes the kinematic body's color mask as well as the dynamic body's. One
kinematic floor under a pile then needs a color per contact: after the 120 dynamic colors the rest
overflow into the serial tail, one 1-lane chunk each (the overflow grouper conflicts them on the
floor's solver body too). Headless pile (`crates/physics/examples/pile.rs`): 186 of 427 contacts
overflowed, 334 chunks a step; patched, none overflow and the layout is the static-wall one (111
chunks).

Why it's sound: a kinematic body's effective inverse mass and inertia are zero
(`update_world_mass_properties`), so a constraint never changes its solver velocity. Every SIMD
lane sharing it gathers and scatters the same value, the case `staged_island_solver/init.rs`'s
lane-disjoint debug check already exempts for non-dynamic bodies. Its contacts now take the top
color band, solved last each sweep like fixed geometry's. With the `parallel` feature (not
enabled here) two workers could store that same unchanged value at once.

Left as upstream: the overflow grouper (`interaction_groups.rs` `group_manifold_refs`) and joint
coloring (`group_interactions`) still conflict on kinematic sides. They only see extra manifolds
of one pair and bodies whose 128 colors are used up, or joints to kinematic bodies.

## Updating

Copy the new crate from the cargo registry over this directory, keep `LICENSE` (refetch it if
upstream changed it) and this file, and re-apply the marked lines. Drop the patch if upstream
colors kinematic bodies as fixed.
