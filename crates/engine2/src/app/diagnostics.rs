//! Process diagnostics managed reports to: memory use, crash-report context, profiler events.

use abi::slots::{engine_glue as glue, error_reports as reports, performance_trace as trace};
use abi::{Registry, bind};

/// Resident set size in bytes; 0 where unsupported.
fn resident_bytes() -> u64 {
    // TODO: Windows (GetProcessMemoryInfo).
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: u64 = statm.split_whitespace().nth(1).and_then(|p| p.parse().ok()).unwrap_or(0);
    pages * 4096
}

pub fn register(r: &mut Registry) {
    bind!(r, glue::APPROXIMATE_PROCESS_MEMORY_USAGE, resident_bytes);
    // Context for Facepunch's crash reporter, which we don't ship; managed logs errors itself.
    bind!(r, reports::BREADCRUMB, |_, _, _| {});
    bind!(r, reports::SET_TAG, |_, _| {});
    // Profiler capture markers; there is no native profiler to receive them.
    bind!(r, trace::BEGIN_EVENT, |_, _, _, _, _, _| {});
    bind!(r, trace::END_EVENT, || {});
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "linux")]
    fn reports_some_memory() {
        assert!(super::resident_bytes() > 1 << 20);
    }
}
