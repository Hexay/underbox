# Differential runs: joints, island sleep, contacts, orbmatch

Joint, sleep and contact findings from `tools/diff/probe/Code/ProbeJoint.cs` and `ProbePush.cs`:
private worlds stepped like a scene (0.02 s, 2 sub-steps), gravity off unless stated. Then the zoo
hoover and orbmatch's bots. Harness and rules: docs/differential.md.

| Marker | What |
| --- | --- |
| `HINGEBODY` | the probe wheel (sphere r = 8): mass 70.29, inertia 1799.4 on both engines |
| `HINGE brake` / `drive` | a hinge velocity motor stopping a spin of 10 rad/s, or driving from rest |
| `HINGE spring` | a hinge spring to 1 rad from rest at 1 Hz (ζ 0 and 1) and 5 Hz (ζ 0.3) |
| `HINGE friction` | hinge friction 1e3–1e7 against a spin of 10 rad/s, and friction set after a motor |
| `OVERLAP` | two 16-unit boxes overlapping by 4, hinged where they meet, joint collision on |
| `CAR` | a box chassis (6 kg) on a slab with three sphere wheels sunk into its underside, hinged, motors at zero speed (max 9.23 and 506.5), joint collision on |
| `STACKSLEEP` / `STACKV` | the step a resting three-box stack and a lone box fall asleep, and their early speeds |
| `PUSHOUT` | two unjointed 16-unit boxes (or radius-16 balls) overlapping by `d`, at rest: their gap and speeds |
| `REAIM` | orbmatch's push: balls at (−400, 0) and (−440, y) set toward the origin every step at 200 and `rear` u/s; `nofriction` zeroes friction |
| `REFRESH` | `ProbeRefresh.cs`: ball B pushed into ball A (stopped before each step) at `vn`, slid at `vt`, optionally spun; A's velocity after the step is the normal used |
| `TRIKE` | `ProbeTrike.cs`: the zoo trike (boxes, two cylinder wheels, two caster spheres, motorised hinges with joint collision) through a 10 × 100 × 25 box sign on a slab, gravity on |
| `MASS` | `ProbeMass.cs`: mass, centre, inertia and density of static, keyframed and dynamic bodies (the sign's box, off-centre), across type changes and overrides; a scene collider and a frozen `Rigidbody` |

## Findings (before → after)

11. **Agreed already. Hinge velocity motors.** Every `HINGE brake` and `drive` marker matched real to
    four decimals before any change: max torque is in kg·in²/s², the target in rad/s, and the motor
    can't exceed its torque over a step. The zoo hoover's motors (speed 0, max 9.2 and 507) are
    not what keeps it moving on real (the zoo hoover, below).
12. **Fixed. Hinge springs.** Real solves a spring twice per substep (again after integrating
    positions, Box2D's relax pass); Rapier once. A model of both reproduces each engine to four
    decimals. Before: 5 Hz ζ 0.3 spun 14.16 rad/s after one step (real 13.43), an undamped 1 Hz
    spring was 4.9° out after a second. After (`joint/build.rs`, extra damping of `h·ω/2` up to
    10 Hz): 13.47, and 1 Hz within 0.2%. Above about 10 Hz the correction overshoots, so stiffer
    springs (wheel suspension) keep the old damping.
13. **Fixed. Hinge friction.** Real ignores it: a 10 rad/s spin keeps exactly 10 at friction 1e7.
    Setting it still clears the motor (`friction after motor`). Ours braked like a motor of that
    torque (at 1e5 the wheel stopped in 10 steps). After: friction does nothing on a hinge
    (`joint::tests::hinge_friction_does_nothing`). Prismatic and ball-socket friction are unprobed.
14. **Fixed. Joints give way to their own contacts.** Real's locked axes are soft: `OVERLAP` ends
    0.8 further apart and holds; `CAR` rises 0.75 and sleeps by step 50. Ours held the joint rigid
    (0.04 apart, jittering; the car sank 0.04 instead). Box2D's 60 Hz ζ 2 fits `OVERLAP` in Rapier but the
    car creeps along at 5 u/s forever. After (locked-axis softness 80 Hz, ζ 40, `joint/build.rs`;
    `joint/soft_tests.rs`): `OVERLAP` 12.39 against real 12.40, `CAR` 8.769 against 8.747, asleep.
    Ours rises over 20 steps where real takes 5, rests 0.5 back along x, and sleeps by step 100
    rather than 50.
15. **Fixed. A stack's island sleeps a step later.** Real puts a resting three-box stack to sleep at
    step 26 and a lone box at 25, though every body is under the threshold from step 1: two
    dynamic bodies that start touching join an island whose timer starts over. Before: all at 25,
    so `DEPTH t=25` showed the stack asleep on ours and awake on real. After
    (`World::restart_sleep_on_new_contacts`, `rest_tests::a_stack_sleeps_a_step_after_a_lone_box`):
    26, 26, 26 and 25, and the `DEPTH` markers match. Real's stack still moves faster on step 1
    (vz −0.95 against −0.20) and slower later; the sinks agree.
16. **Fixed. Contacts persist, and bounce every step.** `REAIM`: real solves each step with the
    normal of the step the pair first touched and keeps it until the contact drifts. Without
    friction the push stays at 13.95° from step 6 to 11, then jumps to the current 17.9° at step
    12 and 22.0° at 17 (drift about 2 units); rolling with friction, it holds 14.03° through step
    22. Ours took a fresh normal every step. Rapier's contact recycling does the same as real, but
    it never recycles a pair with filter hooks, and every shape had them. Real also bounces every
    approach faster than 40–42 u/s (`rear=240` doesn't bounce, `242` does), where Rapier bounces
    only a new contact. Before: front y −4.088 at step 19 (real −3.103), the pair ending each step
    still closing (243.2, 256.7; real 250.55, 249.34). After (`filter.rs`: hooks only on shapes
    some rule can filter, one tag per uncolliding rule, and on jointed bodies; `bounce.rs`): −3.102,
    250.55 and 249.34, positions within 0.04 of real to step 22 with or without friction
    (`orb_tests`). The refresh rule is 18.
    Jointed bodies keep the hooks because recycled contacts freeze their normals in world space
    and fight the soft joint locks (14): the `CAR` crept at 2 u/s and never slept.
17. **Not changed. Push-out of unjointed overlaps.** `PUSHOUT`: the gap already follows real (box
    d = 4: 12.552, 14.096, 15.789 at steps 1, 5, 20 against 12.525, 14.035, 15.978; balls
    identical). Real leaves boxes separating at about 0.35 u/s per unit of overlap (d = 4: 1.40,
    then 2.03 from step 5; d = 12: 6.40; balls 0), so they drift apart and d ≥ 4 never sleeps;
    ours ends at rest. One stabilization (relax) iteration per substep reproduces this (1 PGS: d =
    4 gaps within 0.02, speed 1.33 then 2.24) but breaks the landing, stack depth and sleep, and
    car fits (`landing_tests`, `rest_tests`, `soft_tests`), and moves the zoo trike by under 5
    units. Kept at 2 PGS and 3 stabilization iterations.
18. **Fixed. When a persistent contact is refreshed.** `REFRESH` replays Rapier's recycling bound
    on both engines' poses: a pair keeps its contact while the relative pose drifts at most
    `D = 1.97` (translation plus the rotation chord times an extent) and neither body turns past
    a limit. Pure slides refresh on the same steps on both engines (`vt` 25–200, radius 8–32,
    `vn` 50–200), so `D` agrees. Real's extent is about `r·√2` (fitted 1.35–1.43 r from spins of 2–10
    rad/s at r = 8 and 16; Rapier's is the AABB corner, `r·√3`), and its absolute-turn limit is
    10° (a rolling pair refreshes at 10.4°, not at 9.4°; Rapier's is 11.5°). Both are hard-coded in
    Rapier. The turn limit is what orbmatch's rolling pair hits; `persist.rs` forces a full update
    at 10° by switching the pair's hooks on for one step. The extent is left (it only matters when
    the bodies turn relative to each other while sliding: ours refreshes 1–4 steps early).
    Before: the rolling `REFRESH` pair refreshed at steps 15, 23, 28 (real 13, 20, 25, 29);
    orbmatch's bots parted at tick 23 on real and 26 on ours, `FINAL` `Bot0_0` x = −138.6 and
    `Bot1_1` 42.1 (real −161.5, 52.9). After: 13, 20, 25, 29 (`refresh_tests`); every orbmatch
    `MATCH:POS` within 0.01 of real through tick 70 (`orb_tests` to step 30); `FINAL` −157.5 and
    46.9. Tick 75 differs because a bot scores a tick apart.
19. **Not changed. The trike against its own joints, and through the sign.** `TRIKE` (variants:
    `dynamic` sign, `static` sign, `nojoint`, `nocollide` for joint collision off, `nosign`), and
    headless in `trike_tests::print_trike`, which reproduces ours. Real, alone on the slab
    (`nosign`): the base rises 0.12, the rear wheels slide back 0.7 and all sleep by step 100. Ours
    creeps under its own joints, tips over by step 100 and never sleeps; with joint collision off
    it rests at once. The rear wheels and casters sit inside the base's boxes, so their contacts
    fight the hinges. Through the 819 kg `dynamic` sign, real pushes the base out +x (3.7 by step
    10, 9.9 by 20) and the sign falls flat beside it (z 4.986); the trike ends 16 units out,
    upright and asleep. Ours pushes 1.1–2.0 by step 10, the sign falls onto the trike (z ≈ 9.5)
    and it never sleeps. Tried, none matching both: locked-axis softness 20–160 Hz × ζ 5–40, rigid
    (1 MHz), Box2D's 60 Hz ζ 2, joint warm-starting, friction in the bias pass, max corrective
    velocity 1–20 m/s, dynamic contact softness 10 Hz (`nosign` rests, but the sign then pins the
    trike) or 60 Hz (explodes), and scaling jointed pairs' penetration by 0–0.5 in a contact hook
    (0.25 and below rest `nosign`, but break `OVERLAP` and `CAR` at every softness tried, and the
    sign still lands on the trike). One PGS and one stabilization iteration is the only setting
    that throws the trike out like real (`dynamic`: base x 10–15, sign flat at 4.986) but it
    still creeps alone and breaks 17's fits. Real's static `PhysicsBody.Mass` reads 0 (20).
    **Also tried (not changed): a rigid relax pass.** Box2D solves joints and contacts without
    softness in its relax pass; Rapier keeps a joint's softness (its CFM) in stabilization. At
    our 80 Hz ζ 40 that CFM is 0.5% of the impulse, so Rapier's joints are already near rigid
    there. A rigid sequential-impulse pass after Rapier's step (joints' locked axes and motors,
    with or without the contacts and their friction, 1–20 sweeps), with joint softness 80/40,
    60/2, 30/1, 60/0.7 or 120/5, and Rapier's geometric-mean friction combine, all leave the
    lone trike creeping on its 0.01-friction wheels (`nosign` base x −6 to −13 by step 100,
    −20 to −200 by 1000; real stays within 0.2 and sleeps by 100). The soft settings (30/1,
    60/0.7) do land the `dynamic` sign flat beside the trike (z 4.986, as real) with the base
    pushed +5.4 (real +16.2), but the lone trike still creeps. So the creep is a net force
    from the wheel-in-base contacts against the joints, not the relax pass. Before (`print_trike`):
    `nosign` t = 100 base (0.73, 1.53, 6.06) up 0.69; t = 1000 (15.8, 1.3, 9.4) tipped over.
    `dynamic` t = 100 base (−0.98, 0.33) sign z 9.70 (on the trike). Unchanged.
20. **Fixed. Mass queries on static and keyframed bodies.** `MASS`: a body that isn't dynamic
    reports mass 0, inertia (0, 0, 0) with an identity frame, and its shapes' centroid as its
    mass centre, even with an overridden centre (the override flag still reads true). Its
    mass, centre and inertia overrides are kept and come back when it turns dynamic. A
    dynamic body's inertia is the body-axis diagonal, unsorted, with an identity frame (the
    sign: 725469, 49503, 689622), where Rapier's principal axes come sorted in a rotated frame.
    Density is the first shape's in kg/m³ (2000 by default), 1100 for a body without shapes.
    So a frozen `Rigidbody` (keyframed) reads `Mass` 0 and its `MassOverride` 50 returns on
    unfreezing, as on real. Before: every static and keyframed body reported its shape mass
    (the sign 819.35), inertia (49503, 689622, 725469) at frame (0.5, −0.5, 0.5, −0.5), density
    0.0328 (kg/in³). After (`mass.rs` `reported_*`, used by the body and aggregate slots;
    `mass::tests::only_dynamic_bodies_report_mass_and_inertia`): every `MASS` marker matches, to
    float rounding (inertia 725469.00 against 725469.13).

## The zoo hoover (differential.md, 10)

The zoo's `hoover_a` is a Prop whose package model is missing on both engines, so it gets the
error model: a 10 × 100 × 25 sign hull. Its ModelPhysics trike (`base`, `rear`, two sphere
wheels, three motorised hinges with joint collision) starts inside that sign. `GAME:RESTED`
(logged at `SETTLED` for scenes of 20 bodies or fewer) shows where everything ends:

- Real: the sign falls flat (both engines agree, within 2 units), and the trike is thrown out of it,
  tumbles about 50 units along −y and comes to rest on its side (`up=0.1`) after about 17 s.
- Ours: the sign pushes the trike 9 units out; it stays upright and sleeps within 3 s.

Real's `RESTED`: `base` (583, 138, 8) `up=0.1`, `rear` (581, 132, 12), wheels at y = 138 and
142, `hoover_a` (576, 199, 5). Ours: `base` (575, 183, 0) `up=1.0`, wheels at y = 178, `hoover_a`
(574, 197, 5). Not the motors (11), and not the push-out of unjointed overlaps either: the
`PUSHOUT` probe pushes unjointed boxes apart the way real does (17). Tried, none moving `base` more
than 5 units or tipping it: one stabilization iteration per substep with 1 or 3 PGS iterations
(real's residual push-out speed; 579, 185 and 574, 184), per-step restitution and contact
persistence (16; 576, 184). The trike's bodies are jointed, so they keep fresh contacts (16).
`TRIKE` (19) rebuilds the trike and sign in a private world: two gaps, the trike creeping under its
own joint-vs-contact fight and a weaker push out of the sign. Next: why Rapier's joints never settle
against contacts inside their own bodies (a rigid relax pass is ruled out, 19), then the push-out.
Real's tick-4 sample is
not repeatable either: two runs gave `base` v = 41 and 8, `rear` 12 and 36, because the watcher
samples on wall-clock seconds. Softer joints (Box2D's 60 Hz ζ 2, or 50 Hz ζ 2) kept 4–5 trike
bodies awake for about 20 s, close to real's awake counts, but only because the trike creeps under
its own soft joints, which real's `CAR` doesn't do; rejected for that reason.

## Orbmatch bots (differential.md, 1)

`OrbBot` logs `MATCH:POS` (position and velocity) every 5 ticks. Both engines agree to 0.0001
until the trailing bots rear-end their teammates (tick 5–10). Then, every tick, each bot is
re-aimed at the goal (200 and 300 u/s) and the solver resolves the push:

- Real holds the contact normal of first touch and bounces every approach (16): front 250.55 and
  rear 249.34 u/s at tick 20, front y = −3.103.
- Ours now: 250.55, 249.34, −3.102 (before: 243.2, 256.7, −4.088). Contact softness, PGS and
  stabilization iterations, warm-starting, friction model and restitution alone left tick 20
  within 0.1 of the old numbers; the normal is what moved it.

The rolling pair's contact now refreshes at real's 10° of turn (18): every `MATCH:POS` is within
0.01 of real through tick 70; at 75 a bot scores a tick apart. `FINAL`: `Bot0_0` ends at x =
−157.5 on ours and −161.5 on real (before −138.6, and −180 before 16), `Bot1_1` 46.9 against 52.9.
`orb_tests` replays the push headless (positions and velocities to step 30).
