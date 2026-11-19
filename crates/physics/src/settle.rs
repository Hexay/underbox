//! The per-pair work after Rapier's solve, in one pass over the active contact pairs: record the
//! frozen state (`persist.rs`), bounce approaching points (`bounce.rs`) and list touching pairs
//! (`contacts.rs`). Three passes reloaded the pairs from DRAM each time in a loaded server
//! (docs/perf-bandwidth.md, "Pile: physics working set"). The jobs are independent per pair:
//! bouncing changes velocities only, which neither of the others reads.

use crate::filter::Hooks;
use crate::persist::StepStart;
use crate::world::World;

impl World {
    pub(crate) fn settle_contacts(&mut self, start: StepStart) {
        self.restore_forced_hooks(&start);
        self.frozen.begin();
        self.contacts.begin();
        let hooks = Hooks { rules: &self.rules, meta: &self.meta };
        let raw = &mut self.inner;
        for pair in raw.narrow_phase.contact_pairs().filter(|p| p.has_any_active_contact()) {
            self.frozen.keep(&raw.colliders, &start, pair);
            crate::bounce::bounce_pair(&mut raw.bodies, &self.step_start, pair);
            self.contacts.observe(&raw.colliders, pair, &hooks);
        }
        self.frozen.end();
        self.step_scratch = start;
    }
}
