//! A shader's `RenderState( Name, expr )` declarations at one variant, applied over the draw's own
//! raster state. Values and their `#if` guards evaluate against the variant's defines
//! (`shaders::expr`), so `D_X == 0 ? GREATER_EQUAL : LESS_EQUAL` and `#if D_BLEND` blocks pick
//! their combo's state. Undeclared states keep the draw's; blend and stencil follow D3D11's
//! defaults for the parts left out. `FillMode` is ignored (wgpu's line fill is a native feature).

use render::wgpu::{BlendComponent, BlendFactor as F, BlendOperation as Op, BlendState, ColorWrites, CompareFunction as C, Face, StencilFaceState, StencilOperation as S};
use render::{RasterState, Stencil};
use shaders::expr::{self, Value};
use shaders::{Variant, VfxShader};
use std::collections::HashMap;

/// Each declared state by name, its value evaluated (`Int` for numbers and booleans).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Overrides(HashMap<String, Value>);

/// Every stage's declarations whose guard holds at its variant, later ones winning.
pub fn of(shader: &VfxShader, variant: &dyn Fn(&shaders::Program) -> Variant) -> Overrides {
    let mut out = HashMap::new();
    for p in &shader.programs {
        let defines: HashMap<String, i64> = shaders::compiler::defines(shader, p, &variant(p)).into_iter().filter_map(|(k, v)| Some((k, v.parse().ok()?))).collect();
        let lookup = |n: &str| defines.get(n).copied();
        for (i, (name, value)) in p.decls.render_states.iter().enumerate() {
            let guard = p.decls.render_state_guards.get(i).map_or("", String::as_str);
            if !guard.is_empty() && !expr::eval(guard, &lookup).is_some_and(|g| g.truthy()) {
                continue;
            }
            if let Some(value) = expr::eval(value, &lookup) {
                out.insert(name.clone(), value);
            }
        }
    }
    Overrides(out)
}

fn compare(v: &Value) -> Option<C> {
    Some(match sym(v)? {
        "NEVER" => C::Never,
        "LESS" => C::Less,
        "EQUAL" => C::Equal,
        "LESS_EQUAL" => C::LessEqual,
        "GREATER" => C::Greater,
        "NOT_EQUAL" => C::NotEqual,
        "GREATER_EQUAL" => C::GreaterEqual,
        "ALWAYS" => C::Always,
        _ => return None,
    })
}

/// Declared depth functions are written for native's reverse-Z.
fn forward(c: C) -> C {
    match c {
        C::Less => C::Greater,
        C::LessEqual => C::GreaterEqual,
        C::Greater => C::Less,
        C::GreaterEqual => C::LessEqual,
        c => c,
    }
}

fn factor(v: &Value) -> Option<F> {
    Some(match sym(v)? {
        "ZERO" => F::Zero,
        "ONE" => F::One,
        "SRC_COLOR" => F::Src,
        "INV_SRC_COLOR" => F::OneMinusSrc,
        "SRC_ALPHA" => F::SrcAlpha,
        "INV_SRC_ALPHA" => F::OneMinusSrcAlpha,
        "DEST_ALPHA" => F::DstAlpha,
        "INV_DEST_ALPHA" => F::OneMinusDstAlpha,
        "DEST_COLOR" => F::Dst,
        "INV_DEST_COLOR" => F::OneMinusDst,
        "SRC_ALPHA_SAT" => F::SrcAlphaSaturated,
        "BLEND_FACTOR" => F::Constant,
        "INV_BLEND_FACTOR" => F::OneMinusConstant,
        "SRC1_COLOR" => F::Src1,
        "INV_SRC1_COLOR" => F::OneMinusSrc1,
        "SRC1_ALPHA" => F::Src1Alpha,
        "INV_SRC1_ALPHA" => F::OneMinusSrc1Alpha,
        _ => return None,
    })
}

fn operation(v: &Value) -> Option<Op> {
    Some(match sym(v)? {
        "ADD" => Op::Add,
        "SUBTRACT" => Op::Subtract,
        "REV_SUBTRACT" => Op::ReverseSubtract,
        "MIN" => Op::Min,
        "MAX" => Op::Max,
        _ => return None,
    })
}

fn stencil_op(v: &Value) -> Option<S> {
    Some(match sym(v)? {
        "KEEP" => S::Keep,
        "ZERO" => S::Zero,
        "REPLACE" => S::Replace,
        "INCR_SAT" => S::IncrementClamp,
        "DECR_SAT" => S::DecrementClamp,
        "INVERT" => S::Invert,
        "INCR" => S::IncrementWrap,
        "DECR" => S::DecrementWrap,
        _ => return None,
    })
}

fn cull(v: &Value) -> Option<Option<Face>> {
    Some(match sym(v)? {
        "NONE" => None,
        "BACK" | "DEFAULT" => Some(Face::Back),
        "FRONT" => Some(Face::Front),
        _ => return None,
    })
}

/// `RGBA`, `RGB`, `A`, ... or a number (0 writes nothing).
fn mask(v: &Value) -> ColorWrites {
    let Value::Sym(s) = v else { return ColorWrites::from_bits_truncate(v.int() as u32) };
    let bit = |c| match c {
        'R' => ColorWrites::RED,
        'G' => ColorWrites::GREEN,
        'B' => ColorWrites::BLUE,
        'A' => ColorWrites::ALPHA,
        _ => ColorWrites::empty(),
    };
    s.chars().fold(ColorWrites::empty(), |m, c| m | bit(c))
}

fn sym(v: &Value) -> Option<&str> {
    match v {
        Value::Sym(s) => Some(s),
        Value::Int(_) => None,
    }
}

impl Overrides {
    fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }

    fn flag(&self, name: &str) -> Option<bool> {
        self.get(name).map(Value::truthy)
    }

    fn face(&self, prefix: &str) -> StencilFaceState {
        let op = |n: &str| self.get(&format!("{prefix}{n}")).and_then(stencil_op).unwrap_or(S::Keep);
        let compare = self.get(&format!("{prefix}Func")).and_then(compare).unwrap_or(C::Always);
        StencilFaceState { compare, fail_op: op("FailOp"), depth_fail_op: op("DepthFailOp"), pass_op: op("PassOp") }
    }

    fn blend(&self) -> BlendState {
        let get = |n: &str, f: fn(&Value) -> Option<F>, d| self.get(n).and_then(f).unwrap_or(d);
        let op = |n: &str| self.get(n).and_then(operation).unwrap_or(Op::Add);
        BlendState {
            color: BlendComponent { src_factor: get("SrcBlend", factor, F::One), dst_factor: get("DstBlend", factor, F::Zero), operation: op("BlendOp") },
            alpha: BlendComponent { src_factor: get("SrcBlendAlpha", factor, F::One), dst_factor: get("DstBlendAlpha", factor, F::Zero), operation: op("BlendOpAlpha") },
        }
    }

    fn stencil(&self) -> Stencil {
        let front = self.face("Stencil");
        let two_sided = self.0.keys().any(|k| k.starts_with("BackStencil"));
        let byte = |n: &str| self.get(n).map_or(0xFF, |v| v.int() as u32 & 0xFF);
        Stencil {
            front,
            back: if two_sided { self.face("BackStencil") } else { front },
            read_mask: byte("StencilReadMask"),
            write_mask: byte("StencilWriteMask"),
            reference: self.get("StencilRef").map_or(0, |v| v.int() as u32),
        }
    }

    /// `s` with the declared states; `reverse_z` targets take declared depth functions as is.
    pub fn apply(&self, s: RasterState, reverse_z: bool) -> RasterState {
        let mut s = s;
        if let Some(c) = self.get("DepthFunc").and_then(compare) {
            s.depth_compare = if reverse_z { c } else { forward(c) };
        }
        if let Some(write) = self.flag("DepthWriteEnable") {
            s.depth_write = write;
        }
        if self.flag("DepthEnable") == Some(false) {
            (s.depth_compare, s.depth_write) = (C::Always, false);
        }
        if let Some(c) = self.get("CullMode").and_then(cull) {
            s.cull = c;
        }
        match self.flag("BlendEnable") {
            Some(true) => s.blend = Some(self.blend()),
            Some(false) => s.blend = None,
            None => {}
        }
        if let Some(m) = self.get("ColorWriteEnable0") {
            s.write_mask = mask(m);
        }
        match self.flag("StencilEnable") {
            Some(true) => s.stencil = Some(self.stencil()),
            Some(false) => s.stencil = None,
            None => {}
        }
        if let Some(a2c) = self.flag("AlphaToCoverageEnable") {
            s.alpha_to_coverage = a2c;
        }
        s
    }
}

#[cfg(test)]
#[path = "render_state_tests.rs"]
mod tests;
