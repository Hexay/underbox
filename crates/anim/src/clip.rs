//! Animation clips: keyframes per bone at a fixed rate, sampled with interpolation.

use crate::pose::Pose;
use crate::xform::Xform;

#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub name: String,
    pub fps: f32,
    /// `frames[f][bone]`, parent-relative, in skeleton bone order. A frame may hold fewer
    /// bones than the skeleton; the rest keep the incoming pose.
    pub frames: Vec<Vec<Xform>>,
    pub looping: bool,
    /// Frames are deltas to add to another pose, not a pose themselves.
    pub delta: bool,
    pub interpolate: bool,
    pub hidden: bool,
    /// `morphs[f][controller]` over the model's flex controllers ([`crate::ModelAnim::flex`]);
    /// empty when the clip drives no morphs.
    pub morphs: Vec<Vec<f32>>,
    pub events: Vec<ClipEvent>,
}

/// A named event at a time in the clip, with its payload (`AE_FOOTSTEP {Foot, volume}`...).
#[derive(Debug, Clone, PartialEq)]
pub struct ClipEvent {
    pub time: f32,
    /// Seconds the event spans; 0 for a point event.
    pub duration: f32,
    pub name: String,
    pub data: kv::Value,
}

/// An event a playing clip passed: which, and the clip's cycle (0..1) and time there.
#[derive(Debug, Clone, PartialEq)]
pub struct Fired {
    pub name: String,
    pub data: kv::Value,
    pub cycle: f32,
    pub time: f32,
}

impl Clip {
    pub fn new(name: &str, fps: f32) -> Self {
        Self { name: name.to_owned(), fps, frames: Vec::new(), looping: false, delta: false, interpolate: true, hidden: false, morphs: Vec::new(), events: Vec::new() }
    }

    /// Events in `[from, to)` of unwrapped playback time: every lap of a looping clip, else
    /// clamped to the clip.
    pub fn events_in(&self, from: f32, to: f32, looping: bool) -> Vec<Fired> {
        let d = self.duration();
        if self.events.is_empty() || to <= from {
            return Vec::new();
        }
        let laps = if looping && d > 0.0 { (from / d).floor() as i64..=(to / d).floor() as i64 } else { 0..=0 };
        let mut out = Vec::new();
        for lap in laps {
            for e in &self.events {
                let at = e.time + lap as f32 * d;
                if (from..to).contains(&at) {
                    let cycle = if d > 0.0 { e.time / d } else { 0.0 };
                    out.push(Fired { name: e.name.clone(), data: e.data.clone(), cycle, time: e.time });
                }
            }
        }
        out
    }

    /// Each flex controller's value at `time`; `None` when the clip drives no morphs.
    pub fn morphs_at(&self, time: f32) -> Option<Vec<f32>> {
        self.morphs_at_frame(self.frame_at(time))
    }

    /// [`Clip::morphs_at`] at fractional frame `f` (no wrapping).
    pub fn morphs_at_frame(&self, f: f32) -> Option<Vec<f32>> {
        let last = self.morphs.len().checked_sub(1)?;
        let i = (f.floor() as usize).min(last);
        let j = (i + 1).min(last);
        let t = if self.interpolate { f - i as f32 } else { 0.0 };
        Some(self.morphs[i].iter().zip(&self.morphs[j]).map(|(a, b)| a + (b - a) * t).collect())
    }

    /// Seconds from the first frame to the last: a looping clip's last frame repeats its first.
    pub fn duration(&self) -> f32 {
        if self.fps <= 0.0 { 0.0 } else { self.frames.len().saturating_sub(1) as f32 / self.fps }
    }

    /// Fractional frame for `time`, wrapped if looping, clamped otherwise.
    pub fn frame_at(&self, time: f32) -> f32 {
        let d = self.duration();
        if d <= 0.0 {
            return 0.0;
        }
        let t = if self.looping { time.rem_euclid(d) } else { time.clamp(0.0, d) };
        t * self.fps
    }

    /// Overwrite the bones this clip animates in `pose` with their value at `time`.
    pub fn sample_into(&self, time: f32, pose: &mut Pose) {
        self.sample_frame(self.frame_at(time), pose);
    }

    /// Overwrite the animated bones in `pose` at fractional frame `f` (no wrapping).
    pub fn sample_frame(&self, f: f32, pose: &mut Pose) {
        let Some(last) = self.frames.len().checked_sub(1) else { return };
        let i = (f.floor() as usize).min(last);
        let j = (i + 1).min(last);
        let t = if self.interpolate { f - i as f32 } else { 0.0 };
        for (b, out) in pose.0.iter_mut().enumerate() {
            let (Some(a), Some(c)) = (self.frames[i].get(b), self.frames[j].get(b)) else { continue };
            *out = a.lerp(c, t);
        }
    }

    pub fn sample(&self, time: f32, base: &Pose) -> Pose {
        let mut pose = base.clone();
        self.sample_into(time, &mut pose);
        pose
    }

    /// The root bone's movement from `from` to `to` (seconds, `to` ≥ `from`), counting wraps.
    pub fn root_motion(&self, from: f32, to: f32) -> Xform {
        let at_frame = |f: f32| {
            let mut p = Pose(vec![Xform::IDENTITY]);
            self.sample_frame(f, &mut p);
            p.0[0]
        };
        let root = |t: f32| at_frame(self.frame_at(t));
        let d = self.duration();
        if !self.looping || d <= 0.0 || (from / d).floor() == (to / d).floor() {
            return root(from).to_local(&root(to));
        }
        let (start, end) = (at_frame(0.0), at_frame(self.frames.len() as f32 - 1.0));
        let lap = start.to_local(&end);
        let mut motion = root(from).to_local(&end);
        for _ in 0..(((to / d).floor() - (from / d).floor()) as i32 - 1).max(0) {
            motion = motion.mul(&lap);
        }
        motion.mul(&start.to_local(&root(to)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Quat, Vec3};

    /// Two bones; bone 0 moves 0 → 30 along X over 3 frames at 10 fps.
    fn walk() -> Clip {
        let mut c = Clip::new("walk", 10.0);
        for x in [0.0, 10.0, 30.0] {
            c.frames.push(vec![Xform::new(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY)]);
        }
        c
    }

    #[test]
    fn samples_and_interpolates() {
        let c = walk();
        assert!((c.duration() - 0.2).abs() < 1e-6);
        let base = Pose(vec![Xform::IDENTITY, Xform::new(Vec3::Z, Quat::IDENTITY)]);
        let p = c.sample(0.15, &base);
        assert!((p.0[0].pos.x - 20.0).abs() < 1e-4);
        assert_eq!(p.0[1], base.0[1], "bones the clip lacks keep the incoming pose");
        assert!((c.sample(5.0, &base).0[0].pos.x - 30.0).abs() < 1e-4, "clamps when not looping");
    }

    #[test]
    fn loops_and_steps() {
        let mut c = walk();
        c.looping = true;
        assert!((c.frame_at(0.25) - 0.5).abs() < 1e-4);
        c.interpolate = false;
        let p = c.sample(0.15, &Pose::zero(1));
        assert_eq!(p.0[0].pos.x, 10.0);
    }

    #[test]
    fn morph_tracks_interpolate() {
        let mut c = walk();
        assert_eq!(c.morphs_at(0.1), None);
        c.morphs = vec![vec![0.0], vec![1.0], vec![1.0]];
        assert_eq!(c.morphs_at(0.05), Some(vec![0.5]));
    }

    #[test]
    fn root_motion_accumulates_laps() {
        let mut c = walk();
        assert!((c.root_motion(0.05, 0.15).pos.x - 15.0).abs() < 1e-3);
        c.looping = true;
        // 0.1 → 0.2 (+20), one full lap (+30), 0.0 → 0.1 (+10)
        assert!((c.root_motion(0.1, 0.5).pos.x - 60.0).abs() < 1e-3);
    }
}
