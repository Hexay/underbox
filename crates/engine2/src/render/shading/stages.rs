//! Gotcha: DXC numbers each stage's resources on its own, so a pixel-only texture can land on the
//! binding a vertex-only buffer holds (stand-in: `g_tColor` and `g_TransformBuffer` both at
//! (0, 15)), and each stage's `$Globals` block shares a name and binding but not a layout.
//! [`separate`] moves such pixel bindings past every binding either stage uses and rewrites the
//! pixel module and its reflection to match. Same-named resources with one layout stay shared.

use super::layout::uniform_blocks;
use shaders::CompiledStage;
use shaders::naga::{self, back::wgsl as wgsl_out, valid};
use std::collections::HashMap;

pub fn separate(vs: &CompiledStage, ps: &mut CompiledStage) -> Result<(), String> {
    let taken: HashMap<(u32, u32), &str> = vs.reflection.bindings.iter().map(|b| ((b.group, b.binding), b.name.as_str())).collect();
    let (vs_blocks, ps_blocks) = (uniform_blocks(&vs.wgsl), uniform_blocks(&ps.wgsl));
    let shared = |key: (u32, u32), name: &str| taken.get(&key).is_none_or(|n| *n == name) && vs_blocks.get(&key) == ps_blocks.get(&key).or(vs_blocks.get(&key));
    let mut next: HashMap<u32, u32> = HashMap::new();
    for b in vs.reflection.bindings.iter().chain(&ps.reflection.bindings) {
        let n = next.entry(b.group).or_default();
        *n = (*n).max(b.binding + 1);
    }
    let mut moves = HashMap::new();
    for b in &mut ps.reflection.bindings {
        if !shared((b.group, b.binding), &b.name) {
            let n = next.get_mut(&b.group).expect("every group has a next binding");
            moves.insert((b.group, b.binding), *n);
            b.binding = *n;
            *n += 1;
        }
    }
    if moves.is_empty() {
        return Ok(());
    }
    let mut module = naga::front::wgsl::parse_str(&ps.wgsl).map_err(|e| format!("re-reading pixel stage: {e}"))?;
    for (_, gv) in module.global_variables.iter_mut() {
        if let Some(rb) = gv.binding.as_mut()
            && let Some(to) = moves.get(&(rb.group, rb.binding))
        {
            rb.binding = *to;
        }
    }
    let info = valid::Validator::new(valid::ValidationFlags::all(), valid::Capabilities::all()).validate(&module).map_err(|e| format!("pixel stage after rebinding: {e}"))?;
    ps.wgsl = wgsl_out::write_string(&module, &info, wgsl_out::WriterFlags::empty()).map_err(|e| format!("writing pixel stage: {e}"))?;
    Ok(())
}
