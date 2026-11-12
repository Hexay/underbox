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
