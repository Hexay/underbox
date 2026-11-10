//! Every core shader's default combo (static 0, dynamic 0) through DXC and naga, per stage.
//! Reports pass/fail and the phase each failure stopped in; writes `corpus.tsv` to
//! CARGO_TARGET_TMPDIR. Skips without DXC (tools/fetch-dxc.sh) or an sbox-public checkout.
//! The floors only catch regressions; research/shaders.md "Corpus status" has the analysis.

mod common;

use shaders::{Compiler, Phase, Variant, VfxShader, cache::Cache};
use std::collections::BTreeMap;
use std::sync::Mutex;

const THREADS: usize = 4;

struct Row {
    shader: String,
    stage: String,
    result: Result<(), (Phase, String)>,
}

fn run(core: &std::path::Path, compiler: &Compiler) -> Vec<Row> {
    let includes = common::includes(core);
    let files = common::shader_files(&core.join("shaders"));
    let next = Mutex::new(files.iter());
    let rows = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                while let Some((rel, path)) = { next.lock().unwrap().next() } {
                    let mut out = Vec::new();
                    match VfxShader::parse_file(path, &includes) {
                        Err(e) => out.push(Row { shader: rel.clone(), stage: "-".into(), result: Err((Phase::Dxc, format!("parse: {e}"))) }),
                        Ok(shader) => {
                            for p in &shader.programs {
                                let result = compiler.compile(&shader, p, &Variant::default()).map(|_| ()).map_err(|e| (e.phase, e.message));
                                out.push(Row { shader: rel.clone(), stage: format!("{:?}", p.stage), result });
                            }
                        }
                    }
                    rows.lock().unwrap().extend(out);
                }
            });
        }
    });
    let mut rows = rows.into_inner().unwrap();
    rows.sort_by(|a, b| (&a.shader, &a.stage).cmp(&(&b.shader, &b.stage)));
    rows
}

fn first_line(m: &str) -> String {
    m.lines().find(|l| l.contains("error")).unwrap_or(m.lines().next().unwrap_or("")).chars().take(220).collect()
}

#[test]
fn core_shaders_default_combo() {
    let Some(core) = common::core_dir() else { return eprintln!("no sbox-public checkout; skipped") };
    let mut compiler = match Compiler::new() {
        Ok(c) => c,
        Err(e) => return eprintln!("DXC unavailable ({e}); run tools/fetch-dxc.sh. skipped"),
    };
    compiler.cache = Some(Cache::new(concat!(env!("CARGO_TARGET_TMPDIR"), "/shader-cache")));
    let rows = run(&core, &compiler);

    let full_errors: String =
        rows.iter().filter_map(|r| r.result.as_ref().err().map(|(p, m)| format!("== {} {} {p:?}\n{m}\n", r.shader, r.stage))).collect();
    std::fs::write(concat!(env!("CARGO_TARGET_TMPDIR"), "/corpus-errors.txt"), full_errors).unwrap();
    let mut tsv = String::from("shader\tstage\tresult\tdetail\n");
    let mut by_phase: BTreeMap<String, usize> = BTreeMap::new();
    let mut shaders_ok: BTreeMap<&str, bool> = BTreeMap::new();
    for r in &rows {
        let (status, detail) = match &r.result {
            Ok(()) => ("ok".to_string(), String::new()),
            Err((phase, m)) => (format!("{phase:?}"), first_line(m)),
        };
        *by_phase.entry(status.clone()).or_default() += 1;
        *shaders_ok.entry(&r.shader).or_insert(true) &= r.result.is_ok();
        tsv.push_str(&format!("{}\t{}\t{status}\t{detail}\n", r.shader, r.stage));
    }
    let out = concat!(env!("CARGO_TARGET_TMPDIR"), "/corpus.tsv");
    std::fs::write(out, &tsv).unwrap();
    let past_dxc = rows.iter().filter(|r| !matches!(r.result, Err((Phase::Dxc, _)))).count();
    let full = shaders_ok.values().filter(|ok| **ok).count();
    eprintln!("{tsv}");
    eprintln!("stages: {} total, {past_dxc} through DXC, by result {by_phase:?}", rows.len());
    eprintln!("shaders with every stage through to WGSL: {full}/{}; table in {out}", shaders_ok.len());
    assert!(by_phase.get("ok").copied().unwrap_or(0) >= MIN_OK_STAGES, "fewer stages translate than before");
}

/// Stages that reached validated WGSL when this floor was last raised.
const MIN_OK_STAGES: usize = 53;
