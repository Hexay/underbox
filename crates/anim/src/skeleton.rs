//! Bone hierarchies. Bones are stored parents-first, so one forward pass composes model space.

use crate::xform::Xform;
use glam::Vec3;

#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    /// Bind pose relative to the parent.
    pub local: Xform,
    pub flags: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Skeleton {
    bones: Vec<Bone>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    DuplicateBone(String),
    UnknownParent { bone: String, parent: String },
}

impl Skeleton {
    pub fn bones(&self) -> &[Bone] {
        &self.bones
    }

    pub fn len(&self) -> usize {
        self.bones.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bones.is_empty()
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name.eq_ignore_ascii_case(name))
    }

    pub fn bone(&self, i: usize) -> Option<&Bone> {
        self.bones.get(i)
    }

    /// Append a bone whose bind pose is given in model space (as `CModelBuilder::AddRunTimeBone`
    /// takes it). An empty parent name makes a root.
    pub fn push_model_space(&mut self, name: &str, parent: &str, model: Xform) -> Result<usize, Error> {
        if self.find(name).is_some() {
            return Err(Error::DuplicateBone(name.to_owned()));
        }
        let parent = match parent {
            "" => None,
            p => Some(self.find(p).ok_or_else(|| Error::UnknownParent { bone: name.to_owned(), parent: p.to_owned() })?),
        };
        let local = match parent {
            Some(p) => self.bind_model_space_of(p).to_local(&model),
            None => model,
        };
        self.bones.push(Bone { name: name.to_owned(), parent, local, flags: 0 });
        Ok(self.bones.len() - 1)
    }

    /// Append a bone whose bind pose is relative to its parent.
    pub fn push_local(&mut self, name: &str, parent: Option<usize>, local: Xform) -> usize {
        assert!(parent.is_none_or(|p| p < self.bones.len()), "parents precede children");
        self.bones.push(Bone { name: name.to_owned(), parent, local, flags: 0 });
        self.bones.len() - 1
    }

    pub fn bind_pose(&self) -> Vec<Xform> {
        self.bones.iter().map(|b| b.local).collect()
    }

    fn bind_model_space_of(&self, i: usize) -> Xform {
        let bone = &self.bones[i];
        bone.parent.map_or(bone.local, |p| self.bind_model_space_of(p).mul(&bone.local))
    }

    /// Parent-relative `local` transforms to model space. Bones missing from `local` use bind.
    pub fn model_space(&self, local: &[Xform]) -> Vec<Xform> {
        let mut out: Vec<Xform> = Vec::with_capacity(self.bones.len());
        for (i, bone) in self.bones.iter().enumerate() {
            let l = local.get(i).copied().unwrap_or(bone.local);
            out.push(bone.parent.map_or(l, |p| out[p].mul(&l)));
        }
        out
    }

    /// Model-space transforms back to parent-relative ones.
    pub fn local_space(&self, model: &[Xform]) -> Vec<Xform> {
        self.bones.iter().enumerate().map(|(i, b)| b.parent.map_or(model[i], |p| model[p].to_local(&model[i]))).collect()
    }

    /// The bone whose model-space origin in `bones` is nearest `point`.
    pub fn closest(bones: &[Xform], point: Vec3) -> Option<usize> {
        bones.iter().enumerate().min_by(|(_, a), (_, b)| a.pos.distance_squared(point).total_cmp(&b.pos.distance_squared(point))).map(|(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;
    use std::f32::consts::FRAC_PI_2;

    /// root at origin, `arm` 10 up, rotated 90° about Z; `hand` 5 along arm's local X.
    pub fn arm() -> Skeleton {
        let mut s = Skeleton::default();
        s.push_local("root", None, Xform::IDENTITY);
        s.push_local("arm", Some(0), Xform::new(Vec3::new(0.0, 0.0, 10.0), Quat::from_rotation_z(FRAC_PI_2)));
        s.push_local("hand", Some(1), Xform::new(Vec3::new(5.0, 0.0, 0.0), Quat::IDENTITY));
        s
    }

    #[test]
    fn composes_hierarchy() {
        let s = arm();
        let model = s.model_space(&s.bind_pose());
        assert!(model[2].pos.abs_diff_eq(Vec3::new(0.0, 5.0, 10.0), 1e-5));
        let back = s.local_space(&model);
        assert!(back[2].pos.abs_diff_eq(Vec3::new(5.0, 0.0, 0.0), 1e-5));
    }

    #[test]
    fn model_space_bones_become_local() {
        let mut s = Skeleton::default();
        s.push_model_space("root", "", Xform::new(Vec3::new(1.0, 0.0, 0.0), Quat::IDENTITY)).unwrap();
        s.push_model_space("child", "root", Xform::new(Vec3::new(1.0, 2.0, 0.0), Quat::IDENTITY)).unwrap();
        assert!(s.bone(1).unwrap().local.pos.abs_diff_eq(Vec3::new(0.0, 2.0, 0.0), 1e-6));
        assert_eq!(s.push_model_space("x", "nope", Xform::IDENTITY), Err(Error::UnknownParent { bone: "x".into(), parent: "nope".into() }));
        assert_eq!(s.find("CHILD"), Some(1));
    }

    #[test]
    fn closest_bone() {
        let s = arm();
        let model = s.model_space(&s.bind_pose());
        assert_eq!(Skeleton::closest(&model, Vec3::new(0.0, 4.0, 10.0)), Some(2));
        assert_eq!(Skeleton::closest(&[], Vec3::ZERO), None);
    }
}
