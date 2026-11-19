//! Headless `tools/diff/pile` (docs/perf-physics.md): the same 328 bodies stepped as a scene steps
//! them, timed per step, with a hash of every body's final state for determinism checks.
//! The floor and walls are keyframed, as the scene's collider-only GameObjects are; `static`
//! makes them fixed bodies instead (the cheaper setup the first profile used).
//! usage: cargo run --release -p physics --example pile [ticks=750] [warmup=250] [keyframed|static]
//! `PILE_TRACE=1` prints each measured step's time and awake body count to stderr; `PILE_DRAM`, see
//! [`Dram`].

use physics::{BodyType, Pose, Rotation, Shape, Surface, Vector, World};
use std::time::Instant;

fn add(world: &mut World, kind: BodyType, pos: Vector, rot: Rotation, shape: Shape) -> physics::BodyId {
    let body = world.add_body(kind, Pose::from_parts(pos, rot));
    world.add_shape(body, &shape, Surface::default()).expect("shape");
    body
}

fn cuboid(size: Vector) -> Shape {
    Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: size * 0.5 }
}

/// s&box's `Rotation.From(pitch, yaw, roll)` in degrees.
fn angles(pitch: f32, yaw: f32) -> Rotation {
    Rotation::from_rotation_z(yaw.to_radians()) * Rotation::from_rotation_y(pitch.to_radians())
}

fn build(world: &mut World, walls: BodyType) -> Vec<physics::BodyId> {
    add(world, walls, Vector::new(0.0, 0.0, -8.0), Rotation::IDENTITY, cuboid(Vector::new(1200.0, 1200.0, 16.0)));
    for (x, y) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let size = Vector::new(if x == 0.0 { 1200.0 } else { 16.0 }, if y == 0.0 { 1200.0 } else { 16.0 }, 400.0);
        add(world, walls, Vector::new(600.0 * x, 600.0 * y, 200.0), Rotation::IDENTITY, cuboid(size));
    }
    let mut bodies = Vec::new();
    let mut body = |world: &mut World, pos: Vector, sphere: bool| {
        let shape = if sphere { Shape::Sphere { center: Vector::ZERO, radius: 16.0 } } else { cuboid(Vector::splat(32.0)) };
        bodies.push(add(world, BodyType::Dynamic, pos, angles(pos.x % 30.0, pos.y % 45.0), shape));
    };
    for c in 0..4 {
        for h in 0..10 {
            body(world, Vector::new(-300.0 + 200.0 * c as f32, 300.0, 16.0 + 32.0 * h as f32), false);
        }
    }
    for l in 0..2 {
        for i in 0..12 {
            for j in 0..12 {
                let pos = Vector::new(-440.0 + 80.0 * i as f32 + 20.0 * l as f32, -440.0 + 80.0 * j as f32, 300.0 + 120.0 * l as f32);
                body(world, pos, (i + j + l) % 2 == 0);
            }
        }
    }
    bodies
}

fn mix(h: &mut u64, bits: u32) {
    *h = (*h ^ bits as u64).wrapping_mul(0x100_0000_01b3);
}

fn hash(world: &World, bodies: &[physics::BodyId]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bodies {
        let rb = world.body(b).unwrap();
        let p = rb.position();
        let floats = [p.translation.x, p.translation.y, p.translation.z, p.rotation.x, p.rotation.y, p.rotation.z, p.rotation.w];
        for f in floats.into_iter().chain(rb.linvel().to_array()).chain(rb.angvel().to_array()) {
            mix(&mut h, f.to_bits());
        }
    }
    h
}

fn hash_events(h: &mut u64, world: &mut World) -> usize {
    let events = world.take_contact_events();
    for e in &events {
        let (a, b) = (e.a.into_raw_parts(), e.b.into_raw_parts());
        for bits in [e.kind as u32, a.0, a.1, b.0, b.1, e.impulse.to_bits()].into_iter().chain(e.point.to_array().map(f32::to_bits)) {
            mix(h, bits);
        }
    }
    events.len()
}

/// `PILE_DRAM=<MiB>`: before each step, read that many MiB to evict the caches (as a server's
/// managed work and neighbours do between ticks), then count the step's L2 fills from DRAM on this
/// thread: the bytes a step touches. AMD Zen 5's event, as `tools/diff/perf_events.py`.
struct Dram {
    fd: std::fs::File,
    evict: Vec<u64>,
}

impl Dram {
    fn open() -> Option<Self> {
        use std::os::fd::FromRawFd;
        unsafe extern "C" {
            fn syscall(n: i64, ...) -> i64;
        }
        let mib: usize = std::env::var("PILE_DRAM").ok()?.parse().ok()?;
        // perf_event_attr: type 4 (core PMU), size, config; perf_event_open is syscall 298 on x86_64.
        let mut attr = [0u8; 128];
        attr[0..4].copy_from_slice(&4u32.to_ne_bytes());
        attr[4..8].copy_from_slice(&128u32.to_ne_bytes());
        attr[8..16].copy_from_slice(&0x1_0000_0865u64.to_ne_bytes());
        let fd = unsafe { syscall(298, attr.as_ptr(), 0i32, -1i32, -1i32, 8u64) };
        assert!(fd >= 0, "perf_event_open failed: needs perf_event_paranoid <= 1 and a Zen 5 core PMU");
        Some(Self { fd: unsafe { std::fs::File::from_raw_fd(fd as i32) }, evict: (0..mib << 17).map(|i| i as u64).collect() })
    }

    fn lines(&mut self) -> u64 {
        use std::io::Read;
        let mut b = [0u8; 8];
        self.fd.read_exact(&mut b).expect("read counter");
        u64::from_ne_bytes(b)
    }

    fn evict(&self) {
        std::hint::black_box(self.evict.iter().step_by(8).fold(0u64, |a, &x| a ^ x));
    }
}

/// CPU nanoseconds of every thread in this process.
fn cpu_ns() -> u64 {
    let tasks = std::fs::read_dir("/proc/self/task").map(|d| d.flatten().collect::<Vec<_>>()).unwrap_or_default();
    let stat = |t: &std::fs::DirEntry| std::fs::read_to_string(t.path().join("schedstat")).ok()?.split_whitespace().next()?.parse::<u64>().ok();
    tasks.iter().filter_map(stat).sum()
}

fn main() {
    let arg = |i: usize, default: usize| std::env::args().nth(i).and_then(|a| a.parse().ok()).unwrap_or(default);
    let (ticks, warmup) = (arg(1, 750), arg(2, 250));
    let walls = if std::env::args().nth(3).as_deref() == Some("static") { BodyType::Static } else { BodyType::Keyframed };
    let mut world = World::default();
    let bodies = build(&mut world, walls);
    let mut times = Vec::with_capacity(ticks);
    let (mut dram, mut dram_lines) = (Dram::open(), Vec::with_capacity(ticks));
    let (mut events, mut event_hash, mut cpu_start) = (0, 0xcbf2_9ce4_8422_2325, 0);
    for tick in 1..=ticks {
        if tick == warmup + 1 {
            cpu_start = cpu_ns();
        }
        if tick % 100 == 0 {
            for &b in &bodies {
                let rb = world.body_mut(b).unwrap();
                let v = rb.linvel() + Vector::new(0.0, 0.0, 250.0);
                rb.set_linvel(v, true);
            }
        }
        if let Some(d) = &dram {
            d.evict();
        }
        let before = dram.as_mut().map_or(0, Dram::lines);
        let t = Instant::now();
        world.step(0.02, 1);
        let filled = dram.as_mut().map_or(0, Dram::lines) - before;
        if tick > warmup {
            dram_lines.push(filled);
        }
        let awake = world.take_activity().iter().filter(|a| !a.fell_asleep).count();
        events += hash_events(&mut event_hash, &mut world);
        if tick > warmup {
            times.push(t.elapsed().as_secs_f64() * 1e3);
            if std::env::var_os("PILE_TRACE").is_some() {
                eprintln!("{tick} {:.3} {awake}", times.last().unwrap());
            }
        }
    }
    let cpu = (cpu_ns() - cpu_start) as f64 / 1e6;
    let total: f64 = times.iter().sum();
    times.sort_by(f64::total_cmp);
    let pick = |q: f64| times.get(((times.len() as f64 - 1.0) * q) as usize).copied().unwrap_or(0.0);
    println!(
        "PILE bodies={} walls={walls:?} ticks={} median={:.3}ms p95={:.3}ms wall={total:.1}ms cpu={cpu:.1}ms events={events} hash={:016x} events_hash={event_hash:016x}",
        bodies.len(),
        times.len(),
        pick(0.5),
        pick(0.95),
        hash(&world, &bodies)
    );
    if dram.is_some() {
        dram_lines.sort_unstable();
        let mb = |lines: u64| lines as f64 * 64.0 / 1e6;
        println!("DRAM read MB/step median={:.3} mean={:.3}", mb(dram_lines[dram_lines.len() / 2]), mb(dram_lines.iter().sum::<u64>()) / dram_lines.len() as f64);
    }
}
