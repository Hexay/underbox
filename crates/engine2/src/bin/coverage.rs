//! Slot coverage: how much of `igen_engine` is answered, per native class.
//! usage: cargo run -p engine2 --bin coverage -- [--all] [--stubs] [--json] [--hits server.log]
//! Kinds: subsystem (hand-written), casts and fields (generated), stub (unbound).
//! --hits cross-checks the `stub: NAME` lines a boot logged (tools/boot-server.sh).

use abi::{NAMES, Registry, SLOT_CLASS, SLOT_COUNT, SlotStatus, status};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default, Clone, Copy)]
struct Counts {
    subsystem: usize,
    casts: usize,
    fields: usize,
    stub: usize,
}

impl Counts {
    fn total(&self) -> usize {
        self.subsystem + self.casts + self.fields + self.stub
    }
    fn add(&mut self, o: &Counts) {
        self.subsystem += o.subsystem;
        self.casts += o.casts;
        self.fields += o.fields;
        self.stub += o.stub;
    }
    fn pct(&self) -> f64 {
        100.0 * (self.total() - self.stub) as f64 / self.total().max(1) as f64
    }
    fn json(&self) -> String {
        format!(
            r#"{{"subsystem":{},"casts":{},"fields":{},"stub":{},"total":{}}}"#,
            self.subsystem,
            self.casts,
            self.fields,
            self.stub,
            self.total()
        )
    }
}

fn arg_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}

fn has(flag: &str) -> bool {
    std::env::args().any(|a| a == flag)
}

fn logged_stub_hits(path: &str) -> BTreeSet<String> {
    let log = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    log.lines()
        .filter_map(|l| l.split("stub: ").nth(1))
        .filter_map(|r| r.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

fn main() {
    let mut registry = Registry::default();
    engine2::subsystems::register(&mut registry);
    registry.install();

    let mut classes: BTreeMap<&str, Counts> = BTreeMap::new();
    let mut total = Counts::default();
    for (i, &class) in SLOT_CLASS.iter().enumerate().take(SLOT_COUNT) {
        let class = if class.is_empty() { "(error)" } else { class };
        let c = classes.entry(class).or_default();
        let one = match status(i) {
            SlotStatus::Stub => Counts { stub: 1, ..Default::default() },
            SlotStatus::Bound { owner: "abi::fields" } => Counts { fields: 1, ..Default::default() },
            SlotStatus::Bound { owner: "abi::casts" } => Counts { casts: 1, ..Default::default() },
            SlotStatus::Bound { .. } => Counts { subsystem: 1, ..Default::default() },
        };
        c.add(&one);
        total.add(&one);
    }
    let mut rows: Vec<(&str, Counts)> = classes.into_iter().collect();
    rows.sort_by(|a, b| b.1.stub.cmp(&a.1.stub).then(a.0.cmp(b.0)));

    // Stubs the running game actually hit, and any logged name that is now bound (stale log).
    let hits = arg_value("--hits").map(|p| logged_stub_hits(&p));
    let hit_report = hits.as_ref().map(|h| {
        let still: Vec<&String> =
            h.iter().filter(|n| NAMES.iter().position(|x| x == n).is_some_and(|i| status(i) == SlotStatus::Stub)).collect();
        (h.len(), still)
    });

    if has("--json") {
        let classes: Vec<String> =
            rows.iter().map(|(n, c)| format!(r#"{{"class":"{n}","counts":{}}}"#, c.json())).collect();
        let hits = hit_report.as_ref().map_or("null".into(), |(n, still)| {
            let names: Vec<String> = still.iter().map(|s| format!("\"{s}\"")).collect();
            format!(r#"{{"logged":{n},"still_stub":[{}]}}"#, names.join(","))
        });
        println!(
            r#"{{"total":{},"coverage_pct":{:.2},"classes":[{}],"runtime_hits":{hits}}}"#,
            total.json(),
            total.pct(),
            classes.join(",")
        );
        return;
    }

    println!("| class | stub | subsystem | casts | fields | slots | % |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: |");
    for (class, c) in rows.iter().filter(|(_, c)| has("--all") || c.stub > 0) {
        println!(
            "| {class} | {} | {} | {} | {} | {} | {:.0} |",
            c.stub,
            c.subsystem,
            c.casts,
            c.fields,
            c.total(),
            c.pct()
        );
    }
    println!(
        "\ntotal {SLOT_COUNT}: subsystem {}, casts {}, fields {}, stub {} -> {:.1}% covered",
        total.subsystem,
        total.casts,
        total.fields,
        total.stub,
        total.pct()
    );
    if let Some((n, still)) = &hit_report {
        println!("runtime: {n} stub slots logged as hit, {} still unbound", still.len());
        for s in still {
            println!("  hit-stub {s}");
        }
    }
    if has("--stubs") {
        for i in (0..SLOT_COUNT).filter(|&i| status(i) == SlotStatus::Stub) {
            println!("stub {i} {}", NAMES[i]);
        }
    }
}
