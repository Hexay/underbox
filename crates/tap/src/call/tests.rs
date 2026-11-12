use super::*;
use crate::generated::trampolines::{c33, s2};

extern "C" fn fragments() -> i32 {
    42
}

extern "C" fn real_time(_: f32) {}

#[test]
fn trampolines_forward_and_record() {
    let dir = std::env::temp_dir().join(format!("tap-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    config::install(config::Config::new(dir.clone(), "*", "*", "Sandbox_RealTime_Update", None, 16, None));
    NATIVE[2].store(fragments as *const () as usize, Relaxed);
    MANAGED[33].store(real_time as *const () as usize, Relaxed);

    // SAFETY: the entries above have the trampolines' signatures.
    unsafe {
        assert_eq!(s2(), 42);
        c33(0.25);
        assert_eq!(s2(), 42);
    }
    sink::flush_all();
    let text: String = std::fs::read_dir(&dir).unwrap().map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap()).collect();
    let recs: Vec<Vec<&str>> = text.lines().filter(|l| !l.starts_with('#')).map(|l| l.split('\t').collect()).collect();
    let name = abi::NAMES[2];
    assert_eq!(recs.len(), 3, "{text}");
    assert_eq!(recs[0][1..], ["0", "0", "N", name, "", "42"]);
    assert_eq!(recs[1][1..], ["1", "0", "M", "Sandbox_RealTime_Update", "0.25", "-"]);
    assert_eq!(recs[2][1..], ["1", "0", "N", name, "", "42"]);
    let _ = std::fs::remove_dir_all(&dir);
}
