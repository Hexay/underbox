//! Which way depth runs in a target, the one switch a pass reads its depth clear, test and bias
//! signs from. Native and managed project reverse Z (near = 1): targets clear to 0 and keep the
//! greater depth, and managed's rasterizer biases are negative for it (`r.shadows.depthbias`).

use render::wgpu::CompareFunction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Convention {
    Forward,
    Reverse,
}

/// Shadow maps: managed's shadow mapper projects them with `GetReverseZViewProjTranspose`.
pub const SHADOW: Convention = Convention::Reverse;

/// Camera views and every other target: native's main and skybox views are reverse Z.
pub const VIEW: Convention = Convention::Reverse;

impl Convention {
    pub fn of(reverse_z: bool) -> Convention {
        if reverse_z { Convention::Reverse } else { Convention::Forward }
    }

    pub fn is_reverse(self) -> bool {
        self == Convention::Reverse
    }

    /// The depth a cleared target holds: the far plane.
    pub fn clear(self) -> f32 {
        match self {
            Convention::Forward => 1.0,
            Convention::Reverse => 0.0,
        }
    }

    pub fn compare(self) -> CompareFunction {
        match self {
            Convention::Forward => CompareFunction::LessEqual,
            Convention::Reverse => CompareFunction::GreaterEqual,
        }
    }

    /// A rasterizer bias given in reverse-Z terms (managed's), as this convention applies it:
    /// constant and slope scale as `RasterState::depth_bias` holds them.
    pub fn bias(self, constant: i32, slope: f32) -> (i32, u32) {
        match self {
            Convention::Forward => (-constant, (-slope).to_bits()),
            Convention::Reverse => (constant, slope.to_bits()),
        }
    }
}

/// The stencil tests native's scene layers draw objects with, as real's capture shows them on its
/// `D32FS8` depth (docs/differential-gpu.md, item 3). Every op keeps, so neither writes: the
/// stencil holds what its clear wrote (0) unless an overlay prepass marks it.
pub mod stencil {
    use render::Stencil;
    use render::wgpu::{CompareFunction, StencilFaceState, StencilOperation as Op};

    fn face(compare: CompareFunction) -> StencilFaceState {
        StencilFaceState { compare, fail_op: Op::Keep, depth_fail_op: Op::Keep, pass_op: Op::Keep }
    }

    /// Depth prepasses: only where neither the overlay bit (0x80) nor bit 0 is set.
    pub fn prepass() -> Stencil {
        Stencil { front: face(CompareFunction::Equal), back: face(CompareFunction::Equal), read_mask: 0x81, write_mask: 0, reference: 0 }
    }

    /// Opaque forward layers: front faces where bit 0 is clear.
    pub fn opaque() -> Stencil {
        Stencil { front: face(CompareFunction::NotEqual), back: StencilFaceState::IGNORE, read_mask: 1, write_mask: 0xFE, reference: 3 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reverse_bias_pushes_away_from_the_light_either_way() {
        assert_eq!(Convention::Reverse.bias(-1, -1.5), (-1, (-1.5f32).to_bits()));
        assert_eq!(Convention::Forward.bias(-1, -1.5), (1, 1.5f32.to_bits()));
        assert_eq!((SHADOW.clear(), SHADOW.compare()), (0.0, CompareFunction::GreaterEqual));
    }
}
