//! Texture web streaming (`NativeGlue.Resources`, TextureWebStreaming.cs): native streams a
//! package's texture mips from Facepunch's CDN. We load textures whole from mounted files and
//! stream nothing, so streaming is never active and every prefetch is already done (managed then
//! drops it from its queue instead of retrying). State and cancel stay in render/mod.rs.

use abi::slots::glue_resources as res;
use abi::{Registry, bind};

pub fn register(r: &mut Registry) {
    bind!(r, res::IS_TEXTURE_WEB_STREAMING_ACTIVE, || false);
    bind!(r, res::HAS_TEXTURE_WEB_STREAMING_REQUESTS, || false);
    bind!(r, res::PREFETCH_TEXTURE_WEB_STREAMING, |_path, _url, _size, _generation| true);
}
