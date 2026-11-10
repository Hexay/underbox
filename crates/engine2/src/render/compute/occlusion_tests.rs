use super::*;

/// `CullInstance` words: centre, run, extents (x < 0: never culled), id.
fn instance(run: u32, id: u32) -> [u32; 8] {
    [0, 0, 0, run, 1f32.to_bits(), 1f32.to_bits(), 1f32.to_bits(), id]
}

/// Three phases' copies of `per_phase` args (stride `per_phase.len()`), `ids` ids per phase.
fn cull(instances: &[[u32; 8]], runs: &[[u32; 4]], per_phase: &[[u32; 5]], ids: usize, pass: u32) -> Cull {
    let args: Vec<u32> = (0..3).flat_map(|_| per_phase.iter().flatten().copied()).collect();
    Cull {
        instances: instances.iter().flatten().copied().collect(),
        runs: runs.iter().flatten().copied().collect(),
        args,
        ids: vec![u32::MAX; ids * 3],
        stats: vec![0; 9],
        count: instances.len(),
        pass,
        stride: (per_phase.len(), ids),
        reject_early: false,
        has_previous: false,
    }
}

fn instance_counts(c: &Cull) -> Vec<u32> {
    c.args.chunks(5).map(|a| a[1]).collect()
}

#[test]
fn every_instance_draws_early_and_final_never_late() {
    // One run of two draw calls (LODs' meshes) over instance ids from slot 4.
    let runs = [[4, 0, 2, PHASE_EARLY | PHASE_LATE | PHASE_FINAL]];
    let args = [[36, 0, 0, 0, 0], [12, 0, 36, 0, 0]];
    let instances = [instance(0, 10), instance(0, 11)];

    let mut early = cull(&instances, &runs, &args, 8, 0);
    early.execute();
    assert_eq!(instance_counts(&early), [2, 2, 0, 0, 0, 0]);
    assert_eq!(&early.ids[4..6], [10, 11]);

    let mut late = cull(&instances, &runs, &args, 8, 1);
    late.execute();
    assert_eq!(instance_counts(&late), [0, 0, 0, 0, 2, 2], "late phase empty, final phase both");
    assert_eq!(&late.ids[16 + 4..16 + 6], [10, 11]);
    assert_eq!(late.stats[STAT_VISIBLE], 2);
}

#[test]
fn compacted_fragments_append_their_draws_and_count() {
    // [ survivors ][ fragments ][ count ]: two fragments, final phase only.
    let runs = [[0, 0, 2, PHASE_FINAL | RUN_FRAGMENTS | RUN_COMPACT]];
    let args = [[0; 5], [0; 5], [30, 0, 0, 0, 7], [60, 0, 30, 0, 8], [0; 5]];
    let mut c = cull(&[instance(0, 1), instance(0, 0)], &runs, &args, 0, 1);
    c.execute();
    let phase = &c.args[2 * 25..3 * 25];
    assert_eq!(&phase[0..5], [60, 1, 30, 0, 8], "fragment 1 first, in instance order");
    assert_eq!(&phase[5..10], [30, 1, 0, 0, 7]);
    assert_eq!(phase[20], 2, "survivor count");
    assert_eq!((c.stats[STAT_FRAGMENTS], c.stats[STAT_TRIANGLES]), (2, 30));
}
