//! Direct sequence playback: one clip, its time and looping, with no graph.

use crate::clip::Clip;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Player {
    pub name: String,
    pub time: f32,
    /// `SetSequenceLooping`; `None` follows the clip.
    pub looping: Option<bool>,
    pub blending: bool,
}

impl Player {
    pub fn play(&mut self, name: &str) {
        if !self.name.eq_ignore_ascii_case(name) {
            *self = Player { name: name.to_owned(), looping: self.looping, blending: self.blending, ..Player::default() };
        }
    }

    pub fn looping(&self, clip: &Clip) -> bool {
        self.looping.unwrap_or(clip.looping)
    }

    pub fn advance(&mut self, dt: f32) {
        self.time += dt;
    }

    /// Where playback is through the clip, 0..1 (wrapped when looping).
    pub fn cycle(&self, clip: &Clip) -> f32 {
        let d = clip.duration();
        match d > 0.0 {
            false => 0.0,
            true if self.looping(clip) => (self.time / d).rem_euclid(1.0),
            true => (self.time / d).clamp(0.0, 1.0),
        }
    }

    pub fn set_cycle(&mut self, clip: &Clip, cycle: f32) {
        self.time = cycle * clip.duration();
    }

    pub fn finished(&self, clip: &Clip) -> bool {
        !self.looping(clip) && self.time >= clip.duration()
    }

    /// The time to sample `clip` at, honouring the looping override.
    pub fn sample_time(&self, clip: &Clip) -> f32 {
        self.cycle(clip) * clip.duration()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xform::Xform;

    #[test]
    fn cycles_and_finishes() {
        let mut clip = Clip::new("idle", 10.0);
        clip.frames = vec![vec![Xform::IDENTITY]; 11];
        let mut p = Player::default();
        p.play("idle");
        p.advance(0.25);
        assert!((p.cycle(&clip) - 0.25).abs() < 1e-5);
        p.advance(1.0);
        assert!(p.finished(&clip) && p.cycle(&clip) == 1.0);
        p.looping = Some(true);
        assert!(!p.finished(&clip) && (p.cycle(&clip) - 0.25).abs() < 1e-4);
        p.play("IDLE");
        assert!(p.time > 1.0, "replaying the current sequence keeps its time");
        p.play("run");
        assert_eq!((p.time, p.looping), (0.0, Some(true)));
    }
}
