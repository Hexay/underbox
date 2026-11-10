//! Slot coverage: how much of `igen_engine` is answered, per native class.
//! usage: cargo run -p engine2 --bin coverage [-- --all] [--stubs]
//! (default: only classes with stubs; --stubs also lists every unbound slot)

use abi::{INHERITS, NAMES, Registry, SLOT_CLASS, SLOT_COUNT, SlotStatus, status};
use std::collections::{BTreeMap, HashSet};

fn main() {
    let show_all = std::env::args().any(|a| a == "--all");
    let mut registry = Registry::default();
    engine2::subsystems::register(&mut registry);
    registry.install();

    let inherited: HashSet<usize> = INHERITS.iter().map(|&(d, _)| d).collect();
    let mut classes: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut bound = 0;
    for (i, &class) in SLOT_CLASS.iter().enumerate().take(SLOT_COUNT) {
        let is_bound = status(i) != SlotStatus::Stub;
        bound += usize::from(is_bound);
        let entry = classes.entry(if class.is_empty() { "(error)" } else { class }).or_default();
        entry.0 += usize::from(is_bound);
        entry.1 += 1;
    }

    println!("| class | bound | slots |\n| --- | ---: | ---: |");
    for (class, (b, n)) in &classes {
        if show_all || b < n {
            println!("| {class} | {b} | {n} |");
        }
    }
    let pct = 100.0 * bound as f64 / SLOT_COUNT as f64;
    println!("\n{bound}/{SLOT_COUNT} slots bound ({pct:.1}%); {} inherited slots follow their base class.", inherited.len());
    if std::env::args().any(|a| a == "--stubs") {
        for i in (0..SLOT_COUNT).filter(|&i| status(i) == SlotStatus::Stub) {
            println!("stub {i} {}", NAMES[i]);
        }
    }
}
