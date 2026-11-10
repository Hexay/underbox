//! `g_pResourceSystem`: native's resource loader. Ours loads synchronously on first use (each
//! resource type's `load`), so a "manifest" is just a finished request and nothing is pending.

use abi::slots::g_p_resource_system as rs;
use abi::{Guid, NativeRef, Registry, bind, class};

type Manifest = NativeRef<class::HResourceManifest__>;

/// A load request managed waits on (AsyncResourceLoader); loaded as soon as it exists.
struct Request {
    #[allow(dead_code)]
    path: String,
}

pub fn register(r: &mut Registry) {
    bind!(r, rs::IS_SHUTTING_DOWN, crate::app::platform::shutting_down);
    bind!(r, rs::LOAD_RESOURCE_IN_MANIFEST, |path| -> Manifest {
        objects::create(Request { path: path.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default() })
    });
    bind!(r, rs::IS_MANIFEST_LOADED, |m| objects::get::<_, Request>(m).is_some());
    bind!(r, rs::DESTROY_RESOURCE_MANIFEST, |m| {
        objects::destroy(m);
    });
    bind!(r, rs::LOAD_TEXTURE, |path| crate::render::texture::load(path, Guid::default()));
    bind!(r, rs::HAS_PENDING_WORK, || false);
    bind!(r, rs::GET_NUM_PENDING_STREAMING_REQUESTS, || 0);
    bind!(r, rs::UPDATE_SIMPLE, || {});
    // No GUID database: source files carry none, so managed keys resources by path alone.
    bind!(r, rs::RESOLVE_PATH_TO_GUID, |_, _| false);
    bind!(r, rs::NEEDS_GUID_WRITE, |_| false);
    // TODO: hot reload — evict the path from its type's ByName cache and recompile.
    bind!(r, rs::RELOAD_RESOURCE, |_| {});
    bind!(r, rs::RELOAD_SYMLINKED_RESIDENT_RESOURCES, || {});
    bind!(r, rs::NOTIFY_META_FILE_CHANGED, |_, _| {});
    bind!(r, rs::INVALIDATE_DATABASE, || {});
    // Code manifests list compiled code packages; game code compiles in managed here.
    bind!(r, rs::GET_ALL_CODE_MANIFESTS, |_| {});
}
