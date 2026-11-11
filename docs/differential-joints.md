# Differential runs: joints, island sleep, orbmatch

Joint and sleep findings from `tools/diff/probe/Code/ProbeJoint.cs`: private worlds stepped like a
scene (0.02 s, 2 sub-steps), gravity off unless stated. Then the zoo hoover and orbmatch's bots.
Harness and rules: docs/differential.md.

| Marker | What |
| --- | --- |
| `HINGEBODY` | the probe wheel (sphere r = 8): mass 70.29, inertia 1799.4 on both engines |
| `HINGE brake` / `drive` | a hinge velocity motor stopping a spin of 10 rad/s, or driving from rest |
| `HINGE spring` | a hinge spring to 1 rad from rest at 1 Hz (ζ 0 and 1) and 5 Hz (ζ 0.3) |
| `HINGE friction` | hinge friction 1e3–1e7 against a spin of 10 rad/s, and friction set after a motor |
| `OVERLAP` | two 16-unit boxes overlapping by 4, hinged where they meet, joint collision on |
| `CAR` | a box chassis (6 kg) on a slab with three sphere wheels sunk into its underside, hinged, motors at zero speed (max 9.23 and 506.5), joint collision on |
| `STACKSLEEP` / `STACKV` | the step a resting three-box stack and a lone box fall asleep, and their early speeds |

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

## The zoo hoover (differential.md, 10)

The zoo's `hoover_a` is a Prop whose package model is missing on both engines, so it gets the
error model: a 10 × 100 × 25 sign hull. Its ModelPhysics trike (`base`, `rear`, two sphere
wheels, three motorised hinges with joint collision) starts inside that sign. `GAME:RESTED`
(logged at `SETTLED` for scenes of 20 bodies or fewer) shows where everything ends:

- Real: the sign falls flat (both engines agree, within 2 units), and the trike is thrown out of it,
  tumbles about 50 units along −y and comes to rest on its side (`up=0.1`) after about 17 s.
- Ours: the sign pushes the trike 9 units out; it stays upright and sleeps within 3 s.

So the gap is how deep overlaps between unjointed bodies are pushed apart, not the motors (11). Real's tick-4 sample is
not repeatable either: two runs gave `base` v = 41 and 8, `rear` 12 and 36, because the watcher
samples on wall-clock seconds. Softer joints (Box2D's 60 Hz ζ 2, or 50 Hz ζ 2) kept 4–5 trike
bodies awake for about 20 s, close to real's awake counts, but only because the trike creeps under
its own soft joints, which real's `CAR` doesn't do; rejected for that reason.

## Orbmatch bots (differential.md, 1)

`OrbBot` logs `MATCH:POS` (position and velocity) every 5 ticks. Both engines agree to 0.0001
until the trailing bots rear-end their teammates (tick 5–10). Then, every tick, each bot is
re-aimed at the goal (200 and 300 u/s) and the solver resolves the push:

- Real ends each tick with the pair separating at about 7 u/s along the normal (0.08 of the
  approach), front 250.5 and rear 249.3 u/s at tick 20, and deflects them sideways slowly (front
  y = −3.1 at tick 20).
- Ours ends with the pair still closing (front 243.2, rear 256.7) and deflects them further (−4.1).
  Rapier only bounces a contact on its first step (`is_bouncy`); real bounces every approach.
  Nothing in our integration parameters moves this: contact softness, PGS and stabilisation
  iterations, warm-starting, friction and restitution all leave tick 20 within 0.1.

Positions are what the scripts integrate, and they part by 0.5 by tick 20 and 11 by tick 75, so
`Bot0_0` scores about four ticks later and, after respawning, misses the bump real gives it
(real leaves home pushed sideways, ours runs straight down y = 0 to x = −180); that is the 18.5
at `FINAL`. `orb_tests::rear_end_push_matches_real` (ignored) replays the push headless and
reproduces ours' in-engine numbers exactly. Next: real's contact solve for a pair pushed together
every step (Box2D applies restitution to any approach over its threshold, then relaxes).
