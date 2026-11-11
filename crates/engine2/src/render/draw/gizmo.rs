//! Geometry-shader emulation for the gizmo shaders (gizmo_line, gizmo_sprite), whose `MainGs`
//! naga can't translate. A CPU pre-pass expands each primitive into a quad of corner vertices,
//! two triangles each, carrying what the GS read from its other inputs; the stand-ins
//! (render/shading/standin_gizmo_*.shader) then run the GS's screen-space maths per corner in
//! the vertex shader. Corner encoding, in `Sandbox.Vertex` fields:
//!
//! - line: `tex1` = the segment's other endpoint and the side (±1) to extrude to; `tangent` =
//!   the segment's first endpoint (dash patterns) and 1 on it, 0 on the second;
//! - sprite: `tex1.xy` = the corner (±1, ±1); `tex0` is the point's own size, space and rotation.

use crate::mesh::vertex::Vertex;
use crate::render::shading::standin::Kind;

/// Two triangles over corners laid out as the GS's strip (0 1 2 3).
const QUAD: [u32; 6] = [0, 1, 2, 2, 1, 3];

fn quads(corners: Vec<Vertex>) -> (Vec<Vertex>, Vec<u32>) {
    let indices = (0..corners.len() as u32 / 4).flat_map(|q| QUAD.map(|i| q * 4 + i)).collect();
    (corners, indices)
}

/// `prim`'s segments: line lists pair up, strips chain.
fn segments(prim: i64, v: &[Vertex]) -> Vec<(Vertex, Vertex)> {
    match prim {
        1 | 2 => v.as_chunks::<2>().0.iter().map(|[a, b]| (*a, *b)).collect(),
        3 | 4 => v.windows(2).map(|p| (p[0], p[1])).collect(),
        _ => Vec::new(),
    }
}

fn line_corners(a: Vertex, b: Vertex) -> [Vertex; 4] {
    let corner = |this: Vertex, other: Vertex, side: f32, first: f32| Vertex {
        tex1: [other.position[0], other.position[1], other.position[2], side],
        tangent: [a.position[0], a.position[1], a.position[2], first],
        ..this
    };
    [corner(a, b, 1.0, 1.0), corner(b, a, 1.0, 0.0), corner(a, b, -1.0, 1.0), corner(b, a, -1.0, 0.0)]
}

fn sprite_corners(p: Vertex) -> [Vertex; 4] {
    let corner = |x: f32, y: f32| Vertex { tex1: [x, y, 0.0, 0.0], ..p };
    [corner(-1.0, 1.0), corner(-1.0, -1.0), corner(1.0, 1.0), corner(1.0, -1.0)]
}

/// `vertices` of primitive type `prim` as the triangles `kind`'s stand-in draws; `None` when
/// `kind` isn't a gizmo or `prim` isn't what its GS takes (those draw as submitted).
pub fn expand(kind: Kind, prim: i64, vertices: &[Vertex]) -> Option<(Vec<Vertex>, Vec<u32>)> {
    let corners: Vec<Vertex> = match (kind, prim) {
        (Kind::GizmoLine, 1..=4) => segments(prim, vertices).into_iter().flat_map(|(a, b)| line_corners(a, b)).collect(),
        (Kind::GizmoSprite, 0) => vertices.iter().flat_map(|p| sprite_corners(*p)).collect(),
        _ => return None,
    };
    Some(quads(corners))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32) -> Vertex {
        Vertex { position: [x, 0.0, 0.0], ..Vertex::default() }
    }

    #[test]
    fn a_strip_of_three_is_two_quads_each_knowing_its_far_end() {
        let (corners, indices) = expand(Kind::GizmoLine, 3, &[at(0.0), at(1.0), at(2.0)]).unwrap();
        assert_eq!((corners.len(), indices.len()), (8, 12));
        assert_eq!(&indices[6..], &[4, 5, 6, 6, 5, 7]);
        let second = &corners[4..];
        assert_eq!((second[0].position[0], second[0].tex1[0], second[0].tangent[0]), (1.0, 2.0, 1.0));
        assert_eq!((second[1].position[0], second[1].tex1[0], second[1].tangent[3]), (2.0, 1.0, 0.0));
        assert_eq!(second.iter().map(|c| c.tex1[3]).collect::<Vec<_>>(), [1.0, 1.0, -1.0, -1.0]);
    }

    #[test]
    fn points_become_sprites_and_other_prims_pass_through() {
        let (corners, _) = expand(Kind::GizmoSprite, 0, &[at(5.0)]).unwrap();
        assert!(corners.iter().all(|c| c.position[0] == 5.0));
        assert_eq!(corners.iter().map(|c| [c.tex1[0], c.tex1[1]]).collect::<Vec<_>>(), [[-1.0, 1.0], [-1.0, -1.0], [1.0, 1.0], [1.0, -1.0]]);
        assert!(expand(Kind::GizmoSprite, 5, &[at(0.0)]).is_none());
        assert!(expand(Kind::Unlit, 1, &[at(0.0), at(1.0)]).is_none());
    }
}
