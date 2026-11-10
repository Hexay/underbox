//! Runtime meshes (`MeshGlue`, `CRenderMesh`, `VertexLayout`): the CPU-side data model of
//! vertex and index buffers, layouts, draw calls and morphs, reading model geometry back,
//! serialising model blocks, and the geometry utilities managed calls. GPU upload is the
//! renderer's: it reads [`render_mesh::RenderMesh`], [`buffers::Buffer`] and [`layout::Layout`].

pub mod buffers;
pub mod geometry;
pub mod layout;
pub mod material_ref;
mod model_reads;
pub mod render_mesh;
pub mod serialize;
mod tools;
pub mod vertex;

#[cfg(test)]
mod tests;

pub fn register(r: &mut abi::Registry) {
    layout::register(r);
    buffers::register(r);
    render_mesh::register(r);
    model_reads::register(r);
    tools::register(r);
    serialize::register(r);
}
