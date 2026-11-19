//! Targets for camera renders managed gives none for (depth, a cube face's colour), pooled by size
//! and format across frames. Leased, not shared: concurrent renders of one size would otherwise
//! draw into each other's targets between a clear and the sky.

use crate::render::{swap_chain, texture};
use std::sync::Mutex;

/// Width, height and `ImageFormat`.
type Key = (u32, u32, i32);

static FREE: Mutex<Vec<(Key, texture::Ref)>> = Mutex::new(Vec::new());

pub struct Scratch(pub texture::Ref, Key);

impl Scratch {
    pub fn lease(name: &str, w: u32, h: u32, format: i32) -> Scratch {
        let key = (w, h, format);
        let mut free = FREE.lock().unwrap();
        let t = match free.iter().position(|(k, _)| *k == key) {
            Some(i) => free.swap_remove(i).1,
            None => texture::create_target(name, w, h, format),
        };
        Scratch(t, key)
    }

    pub fn depth(w: u32, h: u32) -> Scratch {
        Scratch::lease("camera depth", w, h, swap_chain::DEPTH_IMAGE_FORMAT)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        FREE.lock().unwrap().push((self.1, self.0));
    }
}

#[cfg(test)]
#[test]
fn concurrent_renders_never_share_a_scratch_target() {
    // An odd size no other test renders at, so the pool's state is this test's.
    let (a, b) = (Scratch::depth(37, 23), Scratch::depth(37, 23));
    assert_ne!(a.0, b.0, "a leased target isn't handed out twice");
    let first = a.0;
    drop(a);
    assert_eq!(Scratch::depth(37, 23).0, first, "a returned target is reused");
    assert_ne!(Scratch::lease("colour", 37, 23, 0).0, first, "formats keep their own targets");
}
