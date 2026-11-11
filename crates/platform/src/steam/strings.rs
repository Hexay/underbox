//! Strings across the steam_api boundary.

use std::ffi::{CStr, CString, c_char};

/// A C string for Steam; interior NULs are dropped.
pub(crate) fn c(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// A C string Steam owns (valid until its next call on that interface), copied out.
pub(crate) fn owned(p: *const c_char) -> String {
    // SAFETY: Steam returns null or a NUL-terminated string.
    if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
}

/// A string Steam writes into a `cap`-byte caller buffer; `None` when `f` says it didn't.
pub(crate) fn written(cap: usize, f: impl FnOnce(*mut c_char, u32) -> bool) -> Option<String> {
    let mut buf = vec![0 as c_char; cap.max(1)];
    let ok = f(buf.as_mut_ptr(), buf.len() as u32);
    *buf.last_mut().unwrap() = 0;
    ok.then(|| owned(buf.as_ptr()))
}

/// [`written`] for calls that always write, "" on failure.
pub(crate) fn buffered(cap: usize, f: impl FnOnce(*mut c_char, u32)) -> String {
    written(cap, |p, n| {
        f(p, n);
        true
    })
    .unwrap_or_default()
}

/// Bytes as lowercase hex (how native hands web tickets to managed).
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        assert_eq!(c("a\0b").as_bytes(), b"ab");
        assert_eq!(owned(std::ptr::null()), "");
        let s = written(4, |p, n| {
            // SAFETY: the buffer has n bytes.
            unsafe { std::ptr::copy_nonoverlapping(c"abcdef".as_ptr(), p, n as usize) };
            true
        });
        assert_eq!(s.as_deref(), Some("abc"), "cut to fit and terminated");
        assert_eq!(written(8, |_, _| false), None);
        assert_eq!(hex(&[0, 0xab, 7]), "00ab07");
    }
}
