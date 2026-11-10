//! Everything animation-related a model carries: skeleton, attachments, hitboxes, clips, and the
//! graph it names.

use crate::clip::Clip;
use crate::skeleton::Skeleton;
use crate::xform::Xform;
use glam::Vec3;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct Influence {
    pub bone: String,
    pub offset: Xform,
    pub weight: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub influences: Vec<Influence>,
    pub ignore_rotation: bool,
}

impl Attachment {
    /// Model (or world) space, given the influencing bones' transforms in that space. Weighted
    /// influences blend in order; unknown bones count as the origin.
    pub fn resolve(&self, bone: impl Fn(&str) -> Option<Xform>) -> Xform {
        let mut out: Option<(Xform, f32)> = None;
        for inf in &self.influences {
            let x = bone(&inf.bone).unwrap_or_default().mul(&inf.offset);
            out = Some(match out {
                None => (x, inf.weight),
                Some((acc, w)) => {
                    let total = w + inf.weight;
                    (acc.lerp(&x, if total > 0.0 { inf.weight / total } else { 0.0 }), total)
                }
            });
        }
        let mut x = out.map(|(x, _)| x).unwrap_or_default();
        if self.ignore_rotation {
            x.rot = glam::Quat::IDENTITY;
        }
        x
    }
}

/// `CHitBox::m_nShapeType` values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HitboxShape {
    Box { min: Vec3, max: Vec3 },
    Sphere { center: Vec3, radius: f32 },
    Capsule { a: Vec3, b: Vec3, radius: f32 },
}

impl HitboxShape {
    pub fn kind(&self) -> u8 {
        match self {
            HitboxShape::Box { .. } => 0,
            HitboxShape::Sphere { .. } => 1,
            HitboxShape::Capsule { .. } => 2,
        }
    }

    /// (`m_vMinBounds`, `m_vMaxBounds`, `m_flShapeRadius`) as managed reads them.
    pub fn fields(&self) -> (Vec3, Vec3, f32) {
        match *self {
            HitboxShape::Box { min, max } => (min, max, 0.0),
            HitboxShape::Sphere { center, radius } => (center, center, radius),
            HitboxShape::Capsule { a, b, radius } => (a, b, radius),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hitbox {
    pub name: String,
    pub bone: String,
    pub surface: String,
    pub tags: Vec<String>,
    pub shape: HitboxShape,
    pub translation_only: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HitboxSet {
    pub name: String,
    pub hitboxes: Vec<Hitbox>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelAnim {
    pub skeleton: Skeleton,
    pub attachments: Vec<Attachment>,
    pub hitbox_sets: Vec<HitboxSet>,
    pub clips: Vec<Arc<Clip>>,
    /// Names of clips the source lists but whose keys we couldn't build (missing FBX).
    pub clip_names: Vec<String>,
    pub graph: Option<String>,
}

impl ModelAnim {
    pub fn clip(&self, name: &str) -> Option<&Arc<Clip>> {
        self.clips.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Every animation name: built clips first, then listed-only ones.
    pub fn animation_names(&self) -> impl Iterator<Item = &str> {
        self.clips.iter().map(|c| c.name.as_str()).chain(self.clip_names.iter().map(String::as_str))
    }

    pub fn attachment(&self, name: &str) -> Option<&Attachment> {
        self.attachments.iter().find(|a| a.name.eq_ignore_ascii_case(name))
    }

    /// An attachment in model space at the bind pose.
    pub fn bind_attachment(&self, a: &Attachment) -> Xform {
        let bones = self.skeleton.model_space(&self.skeleton.bind_pose());
        a.resolve(|n| self.skeleton.find(n).map(|i| bones[i]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    #[test]
    fn attachment_follows_bone_and_blends_influences() {
        let mut m = ModelAnim::default();
        m.skeleton.push_local("root", None, Xform::new(Vec3::new(0.0, 0.0, 10.0), Quat::IDENTITY));
        m.skeleton.push_local("tip", Some(0), Xform::new(Vec3::new(4.0, 0.0, 0.0), Quat::IDENTITY));
        let inf = |bone: &str, w| Influence { bone: bone.into(), offset: Xform::new(Vec3::Y, Quat::IDENTITY), weight: w };
        let a = Attachment { name: "hand".into(), influences: vec![inf("tip", 1.0)], ignore_rotation: false };
        assert!(m.bind_attachment(&a).pos.abs_diff_eq(Vec3::new(4.0, 1.0, 10.0), 1e-5));
        let b = Attachment { name: "mid".into(), influences: vec![inf("root", 1.0), inf("tip", 1.0)], ignore_rotation: true };
        assert!(m.bind_attachment(&b).pos.abs_diff_eq(Vec3::new(2.0, 1.0, 10.0), 1e-5));
    }
}
