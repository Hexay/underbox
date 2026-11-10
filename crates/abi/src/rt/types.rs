//! Safe argument types the generated thunks convert raw ABI values into.

use std::ffi::{CStr, c_char, c_void};
use std::marker::PhantomData;

/// A native object pointer of class `C`, as managed holds it. Opaque: subsystems resolve it through
/// their registries and never dereference it.
#[repr(transparent)]
pub struct NativeRef<C> {
    ptr: *mut c_void,
    _class: PhantomData<fn() -> C>,
}

// Sound: never dereferenced (see above), so it's an address, like a usize.
unsafe impl<C> Send for NativeRef<C> {}
unsafe impl<C> Sync for NativeRef<C> {}

impl<C> NativeRef<C> {
    pub const NULL: Self = Self::from_raw(std::ptr::null_mut());

    pub const fn from_raw(ptr: *mut c_void) -> Self {
        Self { ptr, _class: PhantomData }
    }

    pub fn addr(self) -> usize {
        self.ptr as usize
    }

    pub fn as_raw(self) -> *mut c_void {
        self.ptr
    }

    pub fn is_null(self) -> bool {
        self.ptr.is_null()
    }

    /// Reinterpret as another class; casts between our classes never adjust the pointer.
    pub fn cast<D>(self) -> NativeRef<D> {
        NativeRef::from_raw(self.ptr)
    }
}

impl<C> Clone for NativeRef<C> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<C> Copy for NativeRef<C> {}
impl<C> Default for NativeRef<C> {
    fn default() -> Self {
        Self::NULL
    }
}
impl<C> PartialEq for NativeRef<C> {
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}
impl<C> Eq for NativeRef<C> {}
impl<C> std::hash::Hash for NativeRef<C> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.ptr.hash(state)
    }
}
impl<C> std::fmt::Debug for NativeRef<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NativeRef<{}>({:p})", std::any::type_name::<C>(), self.ptr)
    }
}

/// A pointer whose extent the signature doesn't say: arrays (length in another argument), engine
/// containers (`CUtlVector*`), `void*` buffers, delegates. Reading it is the caller's `unsafe` call.
#[repr(transparent)]
#[derive(Debug)]
pub struct RawPtr<T> {
    ptr: *mut T,
}

impl<T> RawPtr<T> {
    pub const NULL: Self = Self { ptr: std::ptr::null_mut() };

    pub const fn from_raw(ptr: *mut T) -> Self {
        Self { ptr }
    }

    pub fn as_ptr(self) -> *mut T {
        self.ptr
    }

    pub fn cast<U>(self) -> RawPtr<U> {
        RawPtr { ptr: self.ptr.cast() }
    }

    pub fn is_null(self) -> bool {
        self.ptr.is_null()
    }

    /// # Safety
    /// `len` elements must be readable at this pointer for `'a`.
    pub unsafe fn as_slice<'a>(self, len: usize) -> &'a [T] {
        if self.ptr.is_null() || len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(self.ptr, len) } }
    }

    /// # Safety
    /// `len` elements must be writable at this pointer for `'a`, and not aliased.
    pub unsafe fn as_mut_slice<'a>(self, len: usize) -> &'a mut [T] {
        if self.ptr.is_null() || len == 0 { &mut [] } else { unsafe { std::slice::from_raw_parts_mut(self.ptr, len) } }
    }
}

impl<T> Clone for RawPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for RawPtr<T> {}
impl<T> Default for RawPtr<T> {
    fn default() -> Self {
        Self::NULL
    }
}

/// An opaque engine handle that is pointer-sized but not a pointer (`DECLARE_POINTER_HANDLE`).
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PtrHandle<H> {
    pub value: usize,
    _h: PhantomData<fn() -> H>,
}

impl<H> PtrHandle<H> {
    pub const fn new(value: usize) -> Self {
        Self { value, _h: PhantomData }
    }
}

/// An object of a `[Handle:...]` class, crossing as managed's `HandleIndex` id instead of a pointer.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct HandleId<C> {
    pub id: i32,
    _class: PhantomData<fn() -> C>,
}

impl<C> HandleId<C> {
    pub const fn new(id: i32) -> Self {
        Self { id, _class: PhantomData }
    }
}

/// A managed object, addressed by its `Sandbox.InteropSystem` id.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ManagedObject(pub u32);

/// `Sandbox.StringToken`: a murmur2 hash of a string.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct StringToken(pub u32);

/// `System.Guid` / native `UniqueId_t`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Guid(pub [u8; 16]);

/// Read a UTF-8, NUL-terminated string managed passed in; valid for the call.
///
/// # Safety
/// `p` is null or a NUL-terminated string that outlives `'a`.
pub unsafe fn cstr_arg<'a>(p: *const c_char) -> Option<&'a CStr> {
    if p.is_null() { None } else { Some(unsafe { CStr::from_ptr(p) }) }
}
