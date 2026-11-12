//! How one argument or return value prints, so two engines' logs compare: pointers become per-run
//! ordinals, strings are decoded (the game dir as `$GAME`), structs list their fields.

use abi::rt::types::{Guid, HandleId, ManagedObject, PtrHandle, StringToken};
use std::collections::HashMap;
use std::ffi::{CStr, c_char};
use std::fmt::Write;
use std::sync::Mutex;

const MAX_STR: usize = 200;
/// `bytes` hashes at most this much of a buffer.
const MAX_HASHED: usize = 1 << 20;

pub trait Val {
    fn put(&self, l: &mut Line);
}

/// A flattened struct field's type. Not every primitive occurs in today's layouts.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum P {
    U8,
    I8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    Ptr,
}

#[derive(Default)]
pub struct Line {
    pub buf: String,
    sep: bool,
}

impl Line {
    pub fn arg<T: Val + ?Sized>(&mut self, v: &T) {
        self.next();
        v.put(self);
    }

    pub fn next(&mut self) {
        if self.sep {
            self.buf.push_str(", ");
        }
        self.sep = true;
    }

    /// Starts the out-arguments, which print after the call.
    pub fn bar(&mut self) {
        self.buf.push_str(" | ");
        self.sep = false;
    }

    pub fn raw(&mut self, s: &str) {
        self.buf.push_str(s);
    }

    pub fn ptr(&mut self, addr: usize) {
        match addr {
            0 => self.buf.push('0'),
            a => write!(self.buf, "p{}", ordinal(a)).unwrap(),
        }
    }

    pub fn str(&mut self, p: *const c_char) {
        if p.is_null() {
            return self.buf.push_str("null");
        }
        // SAFETY: every `const char*` crossing the table is NUL-terminated and valid for the call.
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy();
        let s = match crate::config::get().and_then(|c| c.game.as_deref()) {
            Some(game) => under_game(&s, game),
            None => s.into_owned(),
        };
        self.buf.push('"');
        for (n, c) in s.chars().enumerate() {
            if n == MAX_STR {
                self.buf.push('…');
                break;
            }
            match c {
                '\t' => self.buf.push_str("\\t"),
                '\n' => self.buf.push_str("\\n"),
                '\r' => self.buf.push_str("\\r"),
                '"' | '\\' => write!(self.buf, "\\{c}").unwrap(),
                c => self.buf.push(c),
            }
        }
        self.buf.push('"');
    }

    /// A buffer as its length and a hash of its first `MAX_HASHED` bytes.
    pub fn bytes(&mut self, p: *const u8, n: usize) {
        if p.is_null() || n == 0 {
            return write!(self.buf, "[{n}]").unwrap();
        }
        // SAFETY: the slot's pointer/length pair describes `n` readable bytes for the call.
        let data = unsafe { std::slice::from_raw_parts(p, n.min(MAX_HASHED)) };
        let h = data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
        write!(self.buf, "[{n}#{:08x}]", h as u32).unwrap();
    }

    pub fn fields(&mut self, base: *const u8, fields: &[(usize, P)], more: bool) {
        self.buf.push('{');
        for (n, &(off, ty)) in fields.iter().enumerate() {
            if n > 0 {
                self.buf.push(' ');
            }
            // SAFETY: offsets and types come from the CLR layout of the struct `base` points at.
            unsafe { self.field(base.add(off), ty) };
        }
        if more {
            self.buf.push_str(" …");
        }
        self.buf.push('}');
    }

    unsafe fn field(&mut self, p: *const u8, ty: P) {
        unsafe {
            match ty {
                P::U8 => p.read().put(self),
                P::I8 => p.cast::<i8>().read().put(self),
                P::I16 => p.cast::<i16>().read_unaligned().put(self),
                P::U16 => p.cast::<u16>().read_unaligned().put(self),
                P::I32 => p.cast::<i32>().read_unaligned().put(self),
                P::U32 => p.cast::<u32>().read_unaligned().put(self),
                P::I64 => p.cast::<i64>().read_unaligned().put(self),
                P::U64 => p.cast::<u64>().read_unaligned().put(self),
                P::F32 => p.cast::<f32>().read_unaligned().put(self),
                P::F64 => p.cast::<f64>().read_unaligned().put(self),
                P::Ptr => self.ptr(p.cast::<usize>().read_unaligned()),
            }
        }
    }
}

/// First-seen order of each address this run. Addresses differ between engines and runs; their
/// ordinals line up as long as objects are created in the same order.
fn ordinal(addr: usize) -> u32 {
    static PTRS: Mutex<Option<HashMap<usize, u32>>> = Mutex::new(None);
    let mut m = PTRS.lock().unwrap_or_else(|e| e.into_inner());
    let m = m.get_or_insert_with(HashMap::new);
    let next = m.len() as u32 + 1;
    *m.entry(addr).or_insert(next)
}

/// `s` with each case-blind occurrence of `game` (lowercase) replaced by `$GAME`.
fn under_game(s: &str, game: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    while let Some(i) = lower[at..].find(game) {
        out.push_str(&s[at..at + i]);
        out.push_str("$GAME");
        at += i + game.len();
    }
    out.push_str(&s[at..]);
    out
}

macro_rules! display {
    ($($t:ty),*) => {$(
        impl Val for $t {
            fn put(&self, l: &mut Line) {
                write!(l.buf, "{self}").unwrap();
            }
        }
    )*};
}
display!(u8, i8, i16, u16, i32, u32, i64, u64, f32, f64);

impl Val for () {
    fn put(&self, l: &mut Line) {
        l.buf.push('-');
    }
}

impl<T> Val for *mut T {
    fn put(&self, l: &mut Line) {
        l.ptr(*self as usize);
    }
}

impl Val for *const c_char {
    fn put(&self, l: &mut Line) {
        l.str(*self);
    }
}

impl<C> Val for HandleId<C> {
    fn put(&self, l: &mut Line) {
        write!(l.buf, "h{}", self.id).unwrap();
    }
}

impl<H> Val for PtrHandle<H> {
    fn put(&self, l: &mut Line) {
        l.ptr(self.value);
    }
}

impl Val for ManagedObject {
    fn put(&self, l: &mut Line) {
        write!(l.buf, "m{}", self.0).unwrap();
    }
}

impl Val for StringToken {
    fn put(&self, l: &mut Line) {
        write!(l.buf, "t{:08x}", self.0).unwrap();
    }
}

impl Val for Guid {
    fn put(&self, l: &mut Line) {
        l.buf.push('g');
        for b in self.0 {
            write!(l.buf, "{b:02x}").unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_print_comparably() {
        let mut l = Line::default();
        l.arg(&1.5f32);
        l.arg(&-3i32);
        l.arg(&c"a\tb".as_ptr());
        l.arg(&std::ptr::null_mut::<u8>());
        l.bar();
        l.arg(&HandleId::<()>::new(7));
        assert_eq!(l.buf, "1.5, -3, \"a\\tb\", 0 | h7");
        let mut l = Line::default();
        l.arg(&abi::structs::Vector3::default());
        assert_eq!(l.buf, "{0 0 0}");
    }

    #[test]
    fn pointers_are_first_seen_ordinals() {
        let (a, b) = (ordinal(0x1000_0001), ordinal(0x1000_0002));
        assert_eq!(ordinal(0x1000_0001), a);
        assert!(b > a);
    }

    #[test]
    fn game_dir_is_folded() {
        assert_eq!(under_game("/G/Run-1/core/a.vmat; /g/run-1/x", "/g/run-1"), "$GAME/core/a.vmat; $GAME/x");
    }

    #[test]
    fn buffers_hash() {
        let mut l = Line::default();
        l.bytes([1u8, 2, 3].as_ptr(), 3);
        l.bytes(std::ptr::null(), 4);
        assert!(l.buf.starts_with("[3#") && l.buf.ends_with("][4]"), "{}", l.buf);
    }
}
