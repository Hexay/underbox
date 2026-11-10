//! `DspPreset` / `DspInstance`: Source-style DSP chains (`DspFactory.CreateBuiltIn` builds them:
//! "tunnel.medium" and friends). A preset is a list of processors with float parameters; an
//! instance runs one chain per channel.

mod units;

pub use units::Unit;

/// The processor ids `DspPreset.Add*` passes (Systems/Audio/Dsp/DspPreset.cs).
pub mod kind {
    pub const DELAY: i32 = 1;
    pub const REVERB: i32 = 2;
    pub const FILTER: i32 = 3;
    pub const MOD_DELAY: i32 = 9;
    pub const DIFFUSOR: i32 = 10;
    pub const AMPLIFIER: i32 = 11;
}

#[derive(Debug, Clone, Default)]
pub struct Preset {
    pub name: String,
    pub processors: Vec<(i32, Vec<f32>)>,
}

pub struct Instance {
    chains: Vec<Vec<Unit>>,
}

impl Instance {
    pub fn new(preset: &Preset, channels: usize) -> Self {
        let chain = || preset.processors.iter().filter_map(|(k, p)| Unit::new(*k, p)).collect();
        Self { chains: (0..channels.max(1)).map(|_| chain()).collect() }
    }

    /// `ProcessChannel`: a channel past the instance's passes through.
    pub fn process(&mut self, channel: usize, input: &crate::Block, output: &mut crate::Block) {
        *output = *input;
        let Some(chain) = self.chains.get_mut(channel) else { return };
        for unit in chain {
            output.iter_mut().for_each(|s| *s = unit.tick(*s));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BLOCK;

    fn preset(processors: Vec<(i32, Vec<f32>)>) -> Preset {
        Preset { name: "test".into(), processors }
    }

    fn impulse_response(p: &Preset, blocks: usize) -> Vec<f32> {
        let mut inst = Instance::new(p, 1);
        let mut out = crate::SILENT;
        let mut ir = Vec::new();
        for b in 0..blocks {
            let mut x = crate::SILENT;
            if b == 0 {
                x[0] = 1.0;
            }
            inst.process(0, &x, &mut out);
            ir.extend_from_slice(&out);
        }
        ir
    }

    #[test]
    fn an_empty_preset_and_unknown_kinds_pass_through() {
        assert_eq!(impulse_response(&preset(vec![(99, vec![1.0])]), 1)[..2], [1.0, 0.0]);
    }

    #[test]
    fn tunnel_medium_rings_after_the_dry_impulse() {
        // DspFactory's "tunnel.medium".
        let p = preset(vec![
            (kind::DIFFUSOR, vec![1.0, 2.0, 0.15, 0.0]),
            (kind::REVERB, vec![100.0, 15.0, 2.0, 0.92, 1.1, 5000.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ]);
        let ir = impulse_response(&p, 40);
        let energy = |s: &[f32]| s.iter().map(|v| v * v).sum::<f32>();
        let early = energy(&ir[BLOCK..BLOCK * 10]);
        let late = energy(&ir[BLOCK * 30..]);
        assert!(early > 0.01, "the reverb is audible: {early}");
        assert!(late < early, "and decays: {late} < {early}");
        assert!(ir.iter().all(|v| v.is_finite() && v.abs() < 4.0));
    }

    #[test]
    fn channels_run_independent_chains() {
        let p = preset(vec![(kind::DELAY, vec![0.0, 1.0, 0.0, 1.0, 0.0, 20000.0, 0.0, 0.0, 0.0, 0.0, 0.0])]);
        let mut inst = Instance::new(&p, 2);
        let mut x = crate::SILENT;
        x[0] = 1.0;
        let mut out = crate::SILENT;
        inst.process(0, &x, &mut out);
        inst.process(1, &crate::SILENT, &mut out);
        assert!(out.iter().all(|&v| v == 0.0), "channel 1 never heard channel 0's impulse");
        inst.process(5, &x, &mut out);
        assert_eq!(out, x);
    }
}
