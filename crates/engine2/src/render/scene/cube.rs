//! `CCameraRenderer.RenderToCubeTexture`: one face per call. Managed sets each face's rotation
//! (`SceneCamera.CubeRotations`, array order) and a 90° field of view, and reads the face back
//! through `GetRenderTargetArrayIndex`, so a face is a plain camera render into array layer
//! `slice`, oriented as any render target is.
//!
//! The view draws into a scratch target of the cube's size and format, copied into the face
//! before every managed callback and once the view is done: managed's filter of the finished cube
//! (`CubemapRendering`, at face 5's `AfterPostProcess`) samples the face as drawn so far.

use super::camera;
use super::scratch::Scratch;
use super::view::SceneView;
use crate::render::{device, swap_chain, texture};

pub(super) fn render(r: camera::Ref, cube: texture::Ref, slice: i32) {
    let Some(t) = texture::resolve(cube) else { return };
    let Some(g) = t.gpu.as_ref() else { return };
    let Some(face) = u32::try_from(slice).ok().filter(|f| *f < g.desc.layers) else { return };
    let (w, h) = (g.desc.width, g.desc.height);
    let (color, depth) = (Scratch::lease("cube face", w, h, t.image_format()), Scratch::depth(w, h));
    camera::render_face(r, swap_chain::Handle::default(), (color.0, depth.0), w, h, Some((cube, face)));
}

/// Copies `view`'s colour into its cube face, when it renders one.
pub(super) fn sync(view: &SceneView) {
    let Some((cube, face)) = view.state.lock().unwrap().cube else { return };
    let (Some(gpu), Some(src), Some(dst)) = (device::gpu(), texture::resolve(view.color), texture::resolve(cube)) else { return };
    if let (Some(src), Some(dst)) = (src.gpu.as_ref(), dst.gpu.as_ref()) {
        src.copy_to_layer(gpu, dst, face);
    }
}

#[cfg(test)]
#[path = "cube_tests.rs"]
mod tests;
