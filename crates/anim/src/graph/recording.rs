//! Recording a graph instance's updates (parameters, time step, character transform) and playing
//! them back: seeking to update `i` restarts the instance and re-runs updates `0..=i` with their
//! recorded parameters, so the pose is the one the instance had then.

use super::Instance;
use crate::params::Params;
use crate::pose::Pose;
use crate::xform::Xform;

/// Managed's `GraphRecording.UpdateKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum UpdateKind {
    /// The first update recorded: it carries the whole parameter state.
    FullState = 2,
    TimeStep = 3,
}

#[derive(Debug, Clone)]
pub struct Update {
    pub kind: UpdateKind,
    pub dt: f32,
    pub params: Params,
    pub character: Xform,
}

/// Managed's `GraphRecording.SeekResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum Seek {
    Success = 0,
    Failure = 2,
}

#[derive(Debug, Default)]
pub struct Recording {
    recording: bool,
    updates: Vec<Update>,
    playback: Option<usize>,
}

impl Recording {
    /// Start a new recording; refused while recording or playing back.
    pub fn start(&mut self) {
        if !self.recording && self.playback.is_none() {
            self.updates.clear();
            self.recording = true;
        }
    }

    pub fn stop(&mut self) {
        self.recording = false;
    }

    pub fn clear(&mut self) {
        if !self.recording && self.playback.is_none() {
            self.updates.clear();
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    pub fn has_data(&self) -> bool {
        !self.updates.is_empty()
    }

    pub fn len(&self) -> usize {
        self.updates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.updates.is_empty()
    }

    /// Note one update the instance just made with `params`.
    pub fn record(&mut self, dt: f32, params: &Params, character: Xform) {
        if self.recording {
            let kind = if self.updates.is_empty() { UpdateKind::FullState } else { UpdateKind::TimeStep };
            self.updates.push(Update { kind, dt, params: params.clone(), character });
        }
    }

    /// Enter playback at the first update; the live instance stops advancing until [`Self::stop_playback`].
    pub fn start_playback(&mut self) -> bool {
        if self.recording || self.updates.is_empty() {
            return false;
        }
        self.playback = Some(0);
        true
    }

    pub fn stop_playback(&mut self) {
        self.playback = None;
    }

    pub fn is_playing(&self) -> bool {
        self.playback.is_some()
    }

    pub fn current_index(&self) -> Option<usize> {
        self.playback
    }

    pub fn current(&self) -> Option<&Update> {
        self.updates.get(self.playback?)
    }

    pub fn get(&self, i: usize) -> Option<&Update> {
        self.updates.get(i)
    }

    /// Replay `instance` up to update `i` during playback and return its pose there.
    pub fn seek(&mut self, i: usize, instance: &mut Instance, bind: &Pose) -> Result<Pose, Seek> {
        if self.playback.is_none() || i >= self.updates.len() {
            return Err(Seek::Failure);
        }
        instance.restart();
        let mut pose = bind.clone();
        for u in &self.updates[..=i] {
            instance.params = u.params.clone();
            pose = instance.update(u.dt, bind);
        }
        self.playback = Some(i);
        Ok(pose)
    }
}
