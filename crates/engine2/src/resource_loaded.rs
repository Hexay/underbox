//! Managed's `Resource.OnResourceLoaded`: real engine2 hands managed every compiled resource file
//! it reads (name without `_c`, the file in memory, its size), on the main thread, so a resource
//! can read its custom blocks (`SoundFile`'s visemes and subtitles). Reads happen anywhere,
//! [nogc] slots and worker threads included, so they queue here and [`flush`] fires them from
//! slots that may call managed: the engine lifecycle and the resource getters.

use std::ffi::CString;
use std::sync::{Mutex, OnceLock};
use std::thread::ThreadId;

const CALLBACK: usize = 44;
/// Real reads only this much of a texture up front; the rest streams in.
const TEXTURE_HEAD: usize = 128 * 1024;

static QUEUE: Mutex<Vec<(CString, Vec<u8>)>> = Mutex::new(Vec::new());
static MAIN: OnceLock<ThreadId> = OnceLock::new();

/// Record a compiled resource file read at `path` (`x.vmat_c`); anything else is ignored.
pub fn read(path: &str, file: &[u8]) {
    let Some(name) = path.strip_suffix("_c") else { return };
    if !abi::rt::callbacks::is_set(CALLBACK) {
        return;
    }
    let Ok(name) = CString::new(name.replace('\\', "/")) else { return };
    let len = if name.to_bytes().ends_with(b".vtex") { file.len().min(TEXTURE_HEAD) } else { file.len() };
    QUEUE.lock().unwrap().push((name, file[..len].to_vec()));
}

/// Make the calling thread the one [`flush`] fires on (managed's main thread).
pub fn set_main_thread() {
    let _ = MAIN.set(std::thread::current().id());
}

/// `result`, after a [`flush`]: for the resource getter slots, which real fires from.
pub fn flushed<R>(result: R) -> R {
    flush();
    result
}

/// Fire every queued read, in order. Only from a slot that isn't [nogc], and only on the main thread.
pub fn flush() {
    if MAIN.get() != Some(&std::thread::current().id()) {
        return;
    }
    loop {
        let batch = std::mem::take(&mut *QUEUE.lock().unwrap());
        if batch.is_empty() {
            return;
        }
        for (name, mut file) in batch {
            let header = abi::RawPtr::from_raw(file.as_mut_ptr().cast());
            abi::callbacks::sandbox_resource::on_resource_loaded(Some(&name), header, file.len() as u64);
        }
    }
}
