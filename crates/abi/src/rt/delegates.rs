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

/// One side of managed's `PhysicsWorld3d.VPhysIntersectionNotification_t`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IntersectionSide {
    pub shape: *mut std::ffi::c_void,
    pub body: *mut std::ffi::c_void,
    pub surface_index: i32,
    pub shape_managed_index: i32,
    pub body_managed_index: i32,
}

/// managed's `PhysicsWorld3d.VPhysIntersectionNotification_t`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IntersectionNotification {
    /// `IntersectionEventType_t`.
    pub reason: i32,
    pub left: IntersectionSide,
    pub right: IntersectionSide,
    pub contact_point: crate::structs::Vector3,
    pub contact_speed: crate::structs::Vector3,
    pub surface_normal: crate::structs::Vector3,
    pub contact_normal_speed: f32,
    pub impulse: f32,
}

/// `IPhysicsWorld.ProcessIntersections`' delegate: managed's `OnIntersections(notifications, count)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntersectionSink(usize);

impl IntersectionSink {
    pub fn from_raw(ptr: crate::RawPtr<std::ffi::c_void>) -> Option<Self> {
        (!ptr.is_null()).then_some(Self(ptr.as_ptr() as usize))
    }

    /// Hand the batch to managed. Calls managed code: hold no lock it could need.
    pub fn deliver(self, batch: &[IntersectionNotification]) {
        if batch.is_empty() {
            return;
        }
        // SAFETY: managed passes a marshalled `ProcessIntersectionsDelegate_t` (PhysicsWorld3d.cs),
        // kept alive in a field, taking (VPhysIntersectionNotification_t*, int).
        let f: unsafe extern "C" fn(*const IntersectionNotification, i32) = unsafe { std::mem::transmute(self.0) };
        unsafe { f(batch.as_ptr(), batch.len() as i32) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_matches_managed_layout() {
        // Sequential: int reason (+4 pad), two 32-byte sides, three Vector3s, two floats.
        assert_eq!(size_of::<IntersectionSide>(), 32);
        assert_eq!(std::mem::offset_of!(IntersectionNotification, left), 8);
        assert_eq!(std::mem::offset_of!(IntersectionNotification, contact_point), 72);
        assert_eq!(std::mem::offset_of!(IntersectionNotification, impulse), 112);
        assert_eq!(size_of::<IntersectionNotification>(), 120);
    }
}
