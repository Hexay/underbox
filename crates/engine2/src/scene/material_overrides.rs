//! A scene object's per-material replacements: by user material attribute (`SetMaterialOverride`,
//! a material whose int attribute matches, in the default skin or the active one), by
//! `Model.Materials` index (`SetMaterialOverrideByIndex`), and the active skin (`SetMaterialGroup`,
//! the default group's slot-for-slot replacement). The whole-object override
//! (`SetMaterialOverrideForMeshInstances`) beats all of them; see `State::material_for`.

use crate::mesh::material_ref::MaterialRef;
use crate::model::Model;
use abi::StringToken;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct Overrides {
    by_attribute: Vec<(StringToken, i32, Arc<MaterialRef>)>,
    by_index: Vec<(usize, Arc<MaterialRef>)>,
    /// The material group's name token; none for the default.
    pub group: Option<StringToken>,
}

/// Whether two handles are one material.
fn same(a: &Arc<MaterialRef>, b: &Arc<MaterialRef>) -> bool {
    Arc::ptr_eq(a, b) || a.material().zip(b.material()).is_some_and(|(a, b)| Arc::ptr_eq(&a, &b))
}

fn attribute(m: &MaterialRef, attr: StringToken) -> Option<i32> {
    m.material()?.attribute(attr).map(|v| v as i32)
}

impl Overrides {
    /// Replace materials whose `attr` is `value`; `None` stops replacing them.
    pub fn set_by_attribute(&mut self, m: Option<Arc<MaterialRef>>, attr: StringToken, value: i32) {
        self.by_attribute.retain(|(a, v, _)| (*a, *v) != (attr, value));
        self.by_attribute.extend(m.map(|m| (attr, value, m)));
    }

    /// Replace `Model.Materials[index]`; `None` stops replacing it.
    pub fn set_by_index(&mut self, index: i32, m: Option<Arc<MaterialRef>>) {
        let Ok(index) = usize::try_from(index) else { return };
        self.by_index.retain(|(i, _)| *i != index);
        self.by_index.extend(m.map(|m| (index, m)));
    }

    /// `ClearMaterialOverrideList`: the skin stays.
    pub fn clear(&mut self) {
        self.by_attribute.clear();
        self.by_index.clear();
    }

    /// The material a draw call of `model` whose own material is `own` draws with.
    pub fn resolve(&self, model: &Model, own: Option<&Arc<MaterialRef>>) -> Option<Arc<MaterialRef>> {
        let own = own?;
        let groups = &model.content.material_groups;
        let skin = self.group.and_then(|t| groups.iter().position(|g| StringToken::of(&g.name) == t)).filter(|&g| g > 0);
        let slot = groups.first().and_then(|g| g.materials.iter().position(|m| same(m, own)));
        let skinned = skin.zip(slot).and_then(|(g, s)| groups[g].materials.get(s)).unwrap_or(own);
        if !self.by_index.is_empty() {
            let listed = crate::model::parts::materials(model);
            if let Some((_, m)) = self.by_index.iter().rev().find(|(i, _)| listed.get(*i).is_some_and(|l| same(l, own))) {
                return Some(m.clone());
            }
        }
        let matches = |attr, value| [own, skinned].into_iter().any(|m| attribute(m, attr) == Some(value));
        let by_attribute = self.by_attribute.iter().rev().find(|(attr, value, _)| matches(*attr, *value));
        Some(by_attribute.map_or(skinned, |(_, _, m)| m).clone())
    }
}

#[cfg(test)]
#[path = "material_overrides_tests.rs"]
mod tests;
