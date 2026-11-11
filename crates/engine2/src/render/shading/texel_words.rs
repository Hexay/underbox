//! The word buffers standing in for the texels of images that shaders use atomically
//! (`<image>_texel_words`, shaders' image_atomics.rs): one per texture, sized to its first mip,
//! kept as long as the texture lives. Only shaders read them, so the texture's own texels stay
//! as they were.

use crate::render::texture::Texture;
use render::wgpu;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

type Words = HashMap<usize, (Weak<Texture>, wgpu::Buffer)>;
static WORDS: LazyLock<Mutex<Words>> = LazyLock::new(Default::default);

/// The image a reflected binding stands in for, when it is a texel word buffer.
pub fn image_of(binding: &str) -> Option<&str> {
    binding.strip_suffix(shaders::TEXEL_WORDS)
}

/// `texture`'s word buffer, created zeroed on first use.
pub fn buffer(gpu: &render::Gpu, texture: &Arc<Texture>) -> Option<wgpu::Buffer> {
    let desc = &texture.gpu.as_ref()?.desc;
    let size = (desc.width as u64 * desc.height as u64 * 4).max(4);
    let mut words = WORDS.lock().unwrap();
    words.retain(|_, (t, _)| t.strong_count() > 0);
    let key = Arc::as_ptr(texture) as usize;
    if let Some((t, b)) = words.get(&key)
        && t.ptr_eq(&Arc::downgrade(texture))
        && b.size() == size
    {
        return Some(b.clone());
    }
    let b = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("texel words"), size, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
    words.insert(key, (Arc::downgrade(texture), b.clone()));
    Some(b)
}
