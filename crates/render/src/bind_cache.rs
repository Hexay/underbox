//! Bind groups reused across frames. A frame rebuilds every draw's groups from the same layout and
//! resources (upload-buffer slices land at the same offsets while draws keep their order), and on
//! lavapipe each new descriptor set maps and frees its own memory. A group is keyed by its layout
//! and its resources' identities and dropped after a frame unused (docs/perf-render.md).

use std::collections::HashMap;
use std::sync::Mutex;
use wgpu::BindingResource as R;

/// Groups cached at once; past it the cache starts over (a caller that never ends frames).
const MAX: usize = 1 << 14;

#[derive(PartialEq, Eq, Hash)]
enum Res {
    Buffer(wgpu::Buffer, u64, Option<wgpu::BufferSize>),
    Sampler(wgpu::Sampler),
    View(wgpu::TextureView),
    Samplers(Vec<wgpu::Sampler>),
    Views(Vec<wgpu::TextureView>),
}

type Key = (wgpu::BindGroupLayout, Vec<(u32, Res)>);

fn res(r: &R) -> Option<Res> {
    Some(match r {
        R::Buffer(b) => Res::Buffer(b.buffer.clone(), b.offset, b.size),
        R::Sampler(s) => Res::Sampler((*s).clone()),
        R::TextureView(v) => Res::View((*v).clone()),
        R::SamplerArray(s) => Res::Samplers(s.iter().map(|s| (*s).clone()).collect()),
        R::TextureViewArray(v) => Res::Views(v.iter().map(|v| (*v).clone()).collect()),
        _ => return None,
    })
}

#[derive(Default)]
pub(crate) struct BindCache {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    groups: HashMap<Key, (wgpu::BindGroup, u64)>,
    frame: u64,
}

impl BindCache {
    pub fn get(&self, device: &wgpu::Device, desc: &wgpu::BindGroupDescriptor) -> wgpu::BindGroup {
        let Some(resources) = desc.entries.iter().map(|e| res(&e.resource).map(|r| (e.binding, r))).collect::<Option<Vec<_>>>() else {
            return device.create_bind_group(desc);
        };
        let mut s = self.state.lock().unwrap();
        if s.groups.len() >= MAX {
            s.groups.clear();
        }
        let frame = s.frame;
        let (group, used) = s.groups.entry((desc.layout.clone(), resources)).or_insert_with(|| (device.create_bind_group(desc), frame));
        *used = frame;
        group.clone()
    }

    /// Drop groups the ending frame didn't use.
    pub fn end_frame(&self) {
        let mut s = self.state.lock().unwrap();
        let frame = s.frame;
        s.groups.retain(|_, (_, used)| *used == frame);
        s.frame += 1;
    }

    /// Forget every group: one created under a failed validation scope may be invalid.
    pub fn clear(&self) {
        self.state.lock().unwrap().groups.clear();
    }
}
