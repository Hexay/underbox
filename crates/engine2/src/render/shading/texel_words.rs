//! The word buffers standing in for the texels of images that shaders use atomically
//! (`<image>_texel_words`, shaders' image_atomics.rs): one per texture, sized to its first mip
//! at `shaders::row_words` per row, kept as long as the texture lives. The texture stays the
//! texels' home between dispatches: [`Synced`] copies it into the words before a dispatch that
//! binds them and back after, so passes that read or write the image directly see the atomics.

use super::bind::{Inputs, names};
use super::program::Shading;
use crate::render::attributes;
use crate::render::texture::{self, Texture};
use render::wgpu;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

type Words = HashMap<usize, (Weak<Texture>, wgpu::Buffer)>;
static WORDS: LazyLock<Mutex<Words>> = LazyLock::new(Default::default);

/// The image a reflected binding stands in for, when it is a texel word buffer.
pub fn image_of(binding: &str) -> Option<&str> {
    binding.strip_suffix(shaders::TEXEL_WORDS)
}

/// The word buffer bound as `binding` (a `<image>_texel_words` name), with its texture.
pub fn bound<P>(gpu: &render::Gpu, s: &Shading<P>, inputs: &Inputs, binding: &str) -> Option<(Arc<Texture>, wgpu::Buffer)> {
    let image = image_of(binding)?;
    let t = inputs.attributes.iter().find_map(|a| names(s, image).find_map(|n| attributes::texture(*a, n))).and_then(texture::resolve)?;
    let words = buffer(gpu, &t)?;
    Some((t, words))
}

/// `texture`'s word buffer, created zeroed on first use.
fn buffer(gpu: &render::Gpu, texture: &Arc<Texture>) -> Option<wgpu::Buffer> {
    let desc = &texture.gpu.as_ref()?.desc;
    let size = (u64::from(shaders::row_words(desc.width)) * u64::from(desc.height) * 4).max(4);
    let mut words = WORDS.lock().unwrap();
    words.retain(|_, (t, _)| t.strong_count() > 0);
    let key = Arc::as_ptr(texture) as usize;
    if let Some((t, b)) = words.get(&key)
        && t.ptr_eq(&Arc::downgrade(texture))
        && b.size() == size
    {
        return Some(b.clone());
    }
    let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
    let b = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("texel words"), size, usage, mapped_at_creation: false });
    words.insert(key, (Arc::downgrade(texture), b.clone()));
    Some(b)
}

/// The word buffers one dispatch binds, with their textures.
pub struct Synced(Vec<(Arc<Texture>, wgpu::Buffer)>);

impl Synced {
    /// The program's word buffers, each loaded from its texture's texels.
    pub fn load<P>(gpu: &render::Gpu, s: &Shading<P>, inputs: &Inputs) -> Synced {
        let pairs: Vec<_> = s.bindings.iter().filter_map(|b| bound(gpu, s, inputs, &b.name)).filter(|(t, _)| copyable(t)).collect();
        copy(gpu, &pairs, true);
        Synced(pairs)
    }

    /// Write the words back into their textures.
    pub fn store(self, gpu: &render::Gpu) {
        copy(gpu, &self.0, false);
    }
}

fn copyable(t: &Texture) -> bool {
    t.gpu.as_ref().is_some_and(|g| g.desc.format.block_copy_size(None) == Some(4))
}

fn copy(gpu: &render::Gpu, pairs: &[(Arc<Texture>, wgpu::Buffer)], into_words: bool) {
    if pairs.is_empty() {
        return;
    }
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("texel words") });
    for (t, words) in pairs {
        let g = t.gpu.as_ref().expect("copyable");
        let texels = wgpu::TexelCopyTextureInfo { texture: &g.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All };
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(shaders::row_words(g.desc.width) * 4), rows_per_image: Some(g.desc.height) };
        let buffer = wgpu::TexelCopyBufferInfo { buffer: words, layout };
        let size = wgpu::Extent3d { width: g.desc.width, height: g.desc.height, depth_or_array_layers: 1 };
        if into_words {
            encoder.copy_texture_to_buffer(texels, buffer, size);
        } else {
            encoder.copy_buffer_to_texture(buffer, texels, size);
        }
    }
    gpu.submit(encoder.finish());
}
