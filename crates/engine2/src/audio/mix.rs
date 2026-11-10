//! Mix buffers: `MIX_BUFFER_SIZE` float samples that managed processors write in place through
//! `GetDataPointer`, and the per-channel sets a device mixes into.

use abi::slots::{c_audio_mix_buffer as buf, c_audio_mix_device_buffers as dev};
use abi::{NativeRef, RawPtr, Registry, bind, class};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// `AudioEngine.MixBufferSize` (Sandbox.Engine/Systems/Audio/AudioEngine.cs).
pub const MIX_BUFFER_SIZE: usize = 512;

type BufRef = NativeRef<class::CAudioMixBuffer>;

/// Samples as f32 bits. Managed writes them through a raw pointer while we may read them, so
/// they're atomics (same layout as f32) rather than plain floats behind a lock.
pub struct MixBuffer(Box<[AtomicU32]>);

impl MixBuffer {
    fn new() -> Self {
        Self((0..MIX_BUFFER_SIZE).map(|_| AtomicU32::new(0)).collect())
    }
    pub fn get(&self, i: usize) -> f32 {
        f32::from_bits(self.0[i].load(Relaxed))
    }
    pub fn set(&self, i: usize, v: f32) {
        self.0[i].store(v.to_bits(), Relaxed)
    }
    fn update(&self, f: impl Fn(usize, f32) -> f32) {
        (0..MIX_BUFFER_SIZE).for_each(|i| self.set(i, f(i, self.get(i))));
    }
}

/// A gain stepping linearly from `from` to `to` across the buffer.
fn ramp(from: f32, to: f32, i: usize) -> f32 {
    from + (to - from) * i as f32 / MIX_BUFFER_SIZE as f32
}

pub struct DeviceBuffers(Vec<BufRef>);

impl Drop for DeviceBuffers {
    fn drop(&mut self) {
        self.0.iter().for_each(|b| {
            objects::destroy(*b);
        });
    }
}

fn on<R: Default>(b: BufRef, f: impl FnOnce(&MixBuffer) -> R) -> R {
    objects::get::<_, MixBuffer>(b).map(|b| f(&b)).unwrap_or_default()
}

/// `f(dst, src)` when both buffers are alive.
fn pair(dst: BufRef, src: BufRef, f: impl FnOnce(&MixBuffer, &MixBuffer)) {
    if let (Some(d), Some(s)) = (objects::get::<_, MixBuffer>(dst), objects::get::<_, MixBuffer>(src)) {
        f(&d, &s)
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, buf::CREATE, || objects::create(MixBuffer::new()));
    bind!(r, buf::DISPOSE, |b| {
        objects::destroy(b);
    });
    bind!(r, buf::GET_DATA_POINTER, |b| on(b, |m| RawPtr::from_raw(m.0.as_ptr() as *mut _)));
    bind!(r, buf::SILENCE, |b| on(b, |m| m.update(|_, _| 0.0)));
    bind!(r, buf::ABS_LEVEL, |b| on(b, |m| (0..MIX_BUFFER_SIZE).map(|i| m.get(i).abs()).fold(0.0, f32::max)));
    bind!(r, buf::AVERGE_LEVEL, |b| {
        on(b, |m| (0..MIX_BUFFER_SIZE).map(|i| m.get(i).abs()).sum::<f32>() / MIX_BUFFER_SIZE as f32)
    });
    bind!(r, buf::RAMP, |b, from, to| on(b, |m| m.update(|i, v| v * ramp(from, to, i))));
    bind!(r, buf::COPY_FROM, |d, s| pair(d, s, |d, s| d.update(|i, _| s.get(i))));
    bind!(r, buf::MIX, |d, s, gain| pair(d, s, |d, s| d.update(|i, v| v + s.get(i) * gain)));
    bind!(r, buf::MIX_RAMP, |d, s, from, to| pair(d, s, |d, s| d.update(|i, v| v + s.get(i) * ramp(from, to, i))));

    bind!(r, dev::CREATE, |channels| {
        let buffers = (0..channels.max(0)).map(|_| objects::create(MixBuffer::new())).collect();
        objects::create(DeviceBuffers(buffers))
    });
    bind!(r, dev::DESTROY, |d| {
        objects::destroy(d);
    });
    bind!(r, dev::GET_BUFFER, |d, i| {
        objects::get::<_, DeviceBuffers>(d)
            .and_then(|d| usize::try_from(i).ok().and_then(|i| d.0.get(i).copied()))
            .unwrap_or_default()
    });
}
