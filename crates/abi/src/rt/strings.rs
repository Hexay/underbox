//! Strings native returns to managed.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};

/// What a slot returns where managed expects a `const char*`. Managed copies the string right
/// after the call (as InteropGen's `SafeReturnString` does), so an owned string only has to live
/// until the next string return on the same thread: nothing is leaked.
#[derive(Debug, Default)]
pub enum StrRet {
    #[default]
    Null,
    Static(&'static CStr),
    Owned(CString),
}

impl From<&'static CStr> for StrRet {
    fn from(s: &'static CStr) -> Self {
        Self::Static(s)
    }
}

impl From<String> for StrRet {
    fn from(s: String) -> Self {
        Self::Owned(to_cstring(s.into_bytes()))
    }
}

impl From<&str> for StrRet {
    fn from(s: &str) -> Self {
        Self::Owned(to_cstring(s.as_bytes().to_vec()))
    }
}

impl<T: Into<StrRet>> From<Option<T>> for StrRet {
    fn from(s: Option<T>) -> Self {
        s.map_or(Self::Null, Into::into)
    }
}

// An interior NUL would truncate in C anyway; cut there instead of failing the call.
fn to_cstring(mut bytes: Vec<u8>) -> CString {
    if let Some(nul) = bytes.iter().position(|&b| b == 0) {
        bytes.truncate(nul);
    }
    CString::new(bytes).expect("interior NULs removed")
}

thread_local! {
    static RETURNED: RefCell<Option<CString>> = const { RefCell::new(None) };
}

impl StrRet {
    pub(crate) fn into_return(self) -> *const c_char {
        match self {
            Self::Null => std::ptr::null(),
            Self::Static(s) => s.as_ptr(),
            Self::Owned(s) => RETURNED.with(|r| r.borrow_mut().insert(s).as_ptr()),
        }
    }
}
