# State differential against real engine2

Part of docs/differential.md (its rules apply). The managed code is identical on both engines, so
any difference in what managed reads back from the scene traces to native.

    tools/diff/run.sh state <corpus|game-checkout> [both|ours|real]
    SCENE=scenes/scenemap.scene tools/diff/run.sh state ../sbox-walker

`state.sh` syncs the corpus project or checkout into a scratch copy under `$UNDERBOX_GAME_COPIES`
(`tools/game/copy.sh`; the source stays read-only), starts it in `SCENE` (default: the project's
startup scene) and injects `tools/diff/StateDump.cs` with its helpers `StateDump*.cs`
(`tools/game/prepare.py`, `--stub-lobby` as `frame.sh`). It boots a dedicated server on ours, then
twice on real (`NOISE=0` once), and runs `state.py`. Ours gets real's compiled core textures,
materials and models (`tools/diff/seed.sh`, `SEED=` on `boot-server.sh`; `CONTENT=source` skips).
`SIDE=client` boots clients instead (`boot-client.sh`; real on Xvfb and lavapipe). On showroom it
adds only the camera's `ScreenRect` and projection: ours renders 1024×768, real's Xvfb is 1280×720.

Before the launchers cut the network off (docs/differential.md, "Rules"), both engines fetched
sandbox's and walker's packages here; real loaded sandbox's map, ours failed to.

Out: `target/diff/state/<name>/{ours,real,real2}.jsonl`, each boot's dir, `report.txt`. A server
boot to tick 200 takes about 15 s once built. Never commit real's dumps.

## What is dumped

At the ticks in `TICKS` (default `1,5,25,50,100,200`), every `EVERY` ticks, and at `LAST`,
counted in StateDump's `OnFixedUpdate`, as `STATE:` JSON log lines `{t, p, k, v}`:

- `k` `#`: each GameObject at path `p` (`A/B`, `[i]` among same-named siblings): name, enabled,
  active, static, flags, tags, world and local transforms, parent, child count, component types.
- `k` the component type (`[i]` for repeats): every public instance property TypeLibrary
  describes. Resources are their path; GameObjects `@path`, components `@path#Type`. A `Model` or
  `SceneObject` one level down gets its own properties (bounds, groups, bone counts, tint).
- `k` `$body[i]`: every body in the scene's PhysicsWorld under its GameObject's path (`$world`
  without one): type, pose, velocities, mass, mass centre, inertia, sleep, damping, bounds, and
  each shape's kind, collider, trigger, friction, surface, tags and triangulated hull.

A getter that throws is recorded as `{"$err": ...}`. The value encoding is in `StateDumpValues.cs`.

## Comparing

`state.py` matches objects by path, components by key, properties by dotted name. Tolerances:
floats `--abs 0.01` plus `--rel 0.001`, vectors per component, rotations by the angle between
quaternions and angles wrapped, `--deg 0.1`. Anything that differs between the two real runs is
dropped first (the noise floor). Each row is one property: first tick, class (`object` or
`component` present on one side only; `kind`, `value`, `number`), score (excess over tolerance),
ticks diverging, ours | real. Then the same divergence grouped across objects.

Validated on showroom, outdoor, skinned, the probe, Facepunch/sandbox (its startup scene; the map
is a package, so neither side loads it) and sbox-walker. Real's noise floor was 0 properties
everywhere except skinned (6: real's `RootMotion` on a server is uninitialized, NaN or 1e29).

## Divergences

Ranked by reach. Before → after where fixed.

Items 1, 2, 4–7 and 10 are fixed, and 3 gives the render bounds, not yet the posed ones
(docs/differential-tap.md, "Fixed"). A walker re-run
(`SCENE=scenes/scenemap.scene`) leaves 116 diverging properties, none of them 1, 2 or 4–7: model
metadata (8), the road cone's hull (20 triangles on ours, 24 on real) and the motion that follows
from it, `Button`'s body bounds 3 units off in x, and the door sleeping by tick 50 on real only.

1. **Fixed. Walker: real's trigger teleports a keyframed coin.** At tick 5 real moves `coin01 (25)` (a
   SphereCollider without a Rigidbody, so keyframed) to the teleport target (4, 200, 4); ours
   leaves it at (−120, −200, 4.5). Before the keyframed-trigger fix (docs/differential-walker.md,
   4) real also moved `Box[0]`, the floor, and the road cone on it fell (vz −51) on real
   only; both now match. This settles that fix's "unverified" case: the trigger does report a
   keyframed collider on real.
2. **Fixed. `SceneObject.Bounds` is model-local on ours** (every renderer, every scene). Real returns the
   model's bounds through the object's transform (showroom's scaled, rotated boxes); ours stores
   the model's bounds at creation and never transforms them (`scene/create.rs`,
   `scene/object_slots.rs` `GET_BOUNDS`). Managed's `LocalBounds` is `Bounds` minus the position, so
   that differs too. `ModelRenderer.Bounds` (managed) agrees.
3. **`SkinnedModelRenderer.Bounds` is zero on ours** (skinned, all three puppets); real gives the
   posed bounds, e.g. (−16, −44, −8)–(16, −12, 24) for the bowing one.
4. **`PhysicsBody.GetBounds` is padded by 0.8 on real**, every axis, for every body (probe,
   walker). A body without shapes (the world body) is an inverted ±3.4e38 box on real, zero on ours.
5. **`PhysicsShape.Friction` is −1 on real** for shapes whose collider sets none; ours reports the
   resolved 0.8.
6. **Shape local bounds are in body space on real.** `Collider.LocalBounds` (from the shapes'
   native local bounds) for a sphere offset from its body: probe `ErrorChair/head` (−6.3, −8,
   44.7)–(9.7, 8, 60.7) on real, ±8 about the origin on ours; walker's coins likewise.
7. **Static bodies sleep on real.** The world body and walker's static MeshComponent bodies report
   `Sleeping` true from tick 1; ours false. Walker's door body sleeps by tick 50 on real only.
8. **Compiled model metadata** (real's own `.vmdl_c`, seeded on both): ours gives body parts 0 (real
   1), body groups 0 (1), material groups 1 (0), so `HasMaterialGroups` flips; `SkeletonName` and
   `Physics` return the model's path on ours, `""` and `embedded_physics.vphys` on real;
   `BoneMaskNames` 0 (1) on the puppet.
9. **Procedural models' names.** Walker's mesh components: `__runtime_model_N` on ours,
   `sbox_procedural_model.vmdl` on real (ResourcePath, Name, ResourceName, ResourceId).
10. **Low.** The error model's inertia has the same principal moments in another axis order and
    frame; a sphere shape triangulates to 62 vertices on ours, 402 on real.

Known already and seen again: the sliding box's vertical pop (docs/differential.md, 4).

## Follow-up lanes

- Skinned bounds (3) from the posed mesh, as real; ours uses the render bounds.
- Compiled model metadata (8, 9), the road cone's hull and `Button`'s bounds.
- `SIDE=client`: one window size on both, then camera, post-processing and render-only state.
