//! A shader that outlasts the wait timeout: the wait returns marked hung instead of blocking,
//! read-backs return zeros, and once the work finishes the GPU recovers. Its own test binary
//! (process), so the busy queue never stalls tests sharing a device.

use render::{Buffer, BufferKind, ComputeProgram, Gpu, Stage, wgpu};
use std::time::{Duration, Instant};

/// An LCG run `n` times on one thread: about a second of lavapipe work per 10⁹ steps.
const WGSL: &str = "
@group(0) @binding(0) var<storage, read_write> data: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var x = 1u;
    for (var i = 0u; i < data[0]; i++) { x = x * 1664525u + 1013904223u; }
    data[1] = x | 1u;
}
";

#[test]
fn a_runaway_shader_times_out_then_recovers() {
    let gpu = Gpu::headless().expect("a wgpu adapter");
    let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }],
    });
    let stage = Stage::new(&gpu, "spin", WGSL, "main");
    let program = ComputeProgram::new(&gpu, "spin", &stage, vec![layout], [1, 1, 1]);
    let steps: u32 = 1_000_000_000;
    let data = Buffer::with_data(&gpu, "data", BufferKind::Storage, &[steps.to_le_bytes(), [0; 4]].concat());
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &program.bind_group_layouts[0],
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: data.buffer.as_entire_binding() }],
    });

    gpu.set_wait_timeout(Duration::from_millis(20));
    program.dispatch(&gpu, std::slice::from_ref(&group), [1, 1, 1]);
    let start = Instant::now();
    assert!(!gpu.wait_idle(), "the wait gives up");
    assert!(gpu.is_hung());
    assert_eq!(data.read(&gpu, 4, 4), [0; 4], "a read-back while hung returns zeros at once");
    // Dropped, not queued behind the running one (a submit there would block until it ends).
    program.dispatch(&gpu, &[group], [1, 1, 1]);
    assert!(start.elapsed() < Duration::from_secs(5), "{:?}", start.elapsed());

    let deadline = Instant::now() + Duration::from_secs(300);
    while !gpu.wait_idle() {
        assert!(Instant::now() < deadline, "the shader never finished");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!gpu.is_hung());
    gpu.set_wait_timeout(Duration::from_secs(30));
    assert_ne!(data.read(&gpu, 4, 4), [0; 4], "the result arrives once it has caught up");
}
