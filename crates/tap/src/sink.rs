//! One line-based log file per thread, `t<NNN>-<thread name>.tap` in the tap dir. Every writer is
//! flushed at each tick and at exit, so a killed run keeps all but its last tick.

use std::cell::RefCell;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

type Writer = Arc<Mutex<BufWriter<File>>>;

static WRITERS: Mutex<Vec<Writer>> = Mutex::new(Vec::new());
static THREADS: AtomicU32 = AtomicU32::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static FULL: AtomicBool = AtomicBool::new(false);

thread_local! {
    static MINE: RefCell<Option<Writer>> = const { RefCell::new(None) };
}

pub fn install_exit_flush() {
    extern "C" fn at_exit() {
        flush_all();
    }
    // SAFETY: registering a plain extern "C" function.
    unsafe { libc::atexit(at_exit) };
}

pub fn full() -> bool {
    FULL.load(Relaxed)
}

pub fn write(record: &str) {
    let Some(cfg) = crate::config::get() else { return };
    let total = BYTES.fetch_add(record.len() as u64, Relaxed) + record.len() as u64;
    let over = total > cfg.max_bytes && !FULL.swap(true, Relaxed);
    let _ = MINE.try_with(|mine| {
        let mut mine = mine.borrow_mut();
        if mine.is_none() {
            *mine = open(&cfg.dir);
        }
        if let Some(w) = mine.as_ref() {
            let mut w = w.lock().unwrap_or_else(|e| e.into_inner());
            let _ = w.write_all(record.as_bytes());
            if over {
                let _ = writeln!(w, "# truncated: UNDERBOX_TAP_MAX_MB reached");
            }
        }
    });
}

pub fn flush_all() {
    for w in WRITERS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        let _ = w.lock().unwrap_or_else(|e| e.into_inner()).flush();
    }
}

fn open(dir: &std::path::Path) -> Option<Writer> {
    let n = THREADS.fetch_add(1, Relaxed);
    let comm = std::fs::read_to_string("/proc/thread-self/comm").unwrap_or_default();
    let name: String = comm.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    let _ = std::fs::create_dir_all(dir);
    let file = File::create(dir.join(format!("t{n:03}-{name}.tap"))).ok()?;
    let mut w = BufWriter::with_capacity(1 << 18, file);
    // SAFETY: gettid has no preconditions.
    let tid = unsafe { libc::gettid() };
    let _ = writeln!(w, "# tap thread={n} name={} tid={tid}\n# seq\ttick\tdepth\tside\tname\targs | outs\tret", comm.trim());
    let w = Arc::new(Mutex::new(w));
    WRITERS.lock().unwrap_or_else(|e| e.into_inner()).push(w.clone());
    Some(w)
}
