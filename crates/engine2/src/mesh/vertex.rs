//! The vertex managed reads model geometry back as (`Sandbox.Vertex`) and the stream bits
//! `MeshGlue.GetDrawCallStreams` reports for it.

/// `Sandbox.Vertex`: sequential, 76 bytes (Resources/Vertex.cs).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [u8; 4],
    pub normal: [f32; 3],
    pub tex0: [f32; 4],
    pub tex1: [f32; 4],
    pub tangent: [f32; 4],
}

const _: () = assert!(size_of::<Vertex>() == 76);

impl Default for Vertex {
    /// Managed's defaults: white, tangent w = -1.
    fn default() -> Self {
        Self { position: [0.0; 3], color: [255; 4], normal: [0.0; 3], tex0: [0.0; 4], tex1: [0.0; 4], tangent: [0.0, 0.0, 0.0, -1.0] }
    }
}

impl From<&assets::model::Vertex> for Vertex {
    fn from(v: &assets::model::Vertex) -> Self {
        let [u, w] = v.uv;
        Self { position: v.position, normal: v.normal, tex0: [u, w, 0.0, 0.0], ..Self::default() }
    }
}

/// Managed's `VertexStreams` flags (Model.VertexData.cs).
pub mod stream {
    pub const POSITION: i32 = 1;
    pub const NORMAL: i32 = 2;
    pub const TANGENT: i32 = 4;
    pub const TEX0: i32 = 8;
    pub const TEX1: i32 = 16;
    pub const COLOR: i32 = 32;
}

impl Vertex {
    pub fn as_bytes(vertices: &[Vertex]) -> &[u8] {
        // SAFETY: Vertex is repr(C) plain data with no padding (76 = 12 + 4 + 12 + 3 * 16).
        unsafe { std::slice::from_raw_parts(vertices.as_ptr().cast(), std::mem::size_of_val(vertices)) }
    }

    /// The vertices in tightly packed `bytes`; a trailing partial vertex is dropped.
    pub fn from_bytes(bytes: &[u8]) -> Vec<Vertex> {
        // SAFETY: each chunk is one Vertex's bytes, and every bit pattern is a valid Vertex.
        bytes.chunks_exact(size_of::<Vertex>()).map(|c| unsafe { c.as_ptr().cast::<Vertex>().read_unaligned() }).collect()
    }
}
