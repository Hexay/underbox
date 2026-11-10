//! Function pointers managed puts in structs for native to call back (not table callbacks).

/// `PhysicsTrace.Request.FilterDelegate`: whether a trace may hit the shape with managed handle
/// `shape`. Zero means no filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceFilter(usize);

impl TraceFilter {
    pub fn from_raw(ptr: usize) -> Option<Self> {
        (ptr != 0).then_some(Self(ptr))
    }

    /// Ask managed. Calls managed code: hold no lock it could need.
    pub fn allows(self, shape: i32) -> bool {
        // SAFETY: managed sets FilterDelegate to `&FilterFunctionInternal`, an
        // [UnmanagedCallersOnly] `byte (int)` that outlives the trace (PhysicsWorld3d.Trace.cs).
        let f: unsafe extern "C" fn(i32) -> u8 = unsafe { std::mem::transmute(self.0) };
        unsafe { f(shape) != 0 }
    }
}
