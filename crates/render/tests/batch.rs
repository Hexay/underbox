//! The shared upload buffer under threads: one thread's frame end must not rewind the buffer over
//! slices another thread has staged but not yet submitted (src/batch.rs).

use render::{Buffer, BufferKind, Gpu, wgpu};
use std::sync::Barrier;

const LEN: usize = 256;

#[test]
fn a_frame_end_keeps_another_threads_unsubmitted_slices() {
    let gpu = Gpu::headless().expect("a wgpu adapter (install mesa-vulkan-drivers for lavapipe)");
    let dst = Buffer::new(&gpu, "dst", BufferKind::Storage, LEN as u64);
    let (staged, rewound) = (Barrier::new(2), Barrier::new(2));
    std::thread::scope(|s| {
        s.spawn(|| {
            let slice = gpu.upload(&[1; LEN], 0);
            staged.wait();
            rewound.wait();
            let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            encoder.copy_buffer_to_buffer(&slice.buffer, slice.offset, &dst.buffer, 0, LEN as u64);
            gpu.submit(encoder.finish());
        });
        staged.wait();
        gpu.end_frame();
        gpu.upload(&[2; LEN], 0);
        rewound.wait();
    });
    let read = dst.read(&gpu, 0, LEN as u64);
    assert!(read.iter().all(|b| *b == 1), "the slice was overwritten: {:?}", &read[..8]);
}
