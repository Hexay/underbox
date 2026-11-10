//! s&box's `igen_engine` interop table as safe Rust. See docs/ARCHITECTURE.md#slots.
//!
//! - [`slots`]: a typed [`Slot`] per table entry, grouped by native class (`slots::i_physics_body::SET_MASS`).
//! - [`Registry`]: subsystems [`bind!`] safe Rust functions to slots; unbound slots answer zero and are counted.
//! - [`callbacks`]: the managed functions native may call.
//! - [`structs`]: every struct that crosses, laid out as the CLR lays it out (asserted at compile time).

mod generated;
pub mod rt;

pub use generated::callbacks;
pub use generated::slots::{self, CALLBACK_COUNT, HASH, INHERITS, NAMES, SLOT_CLASS, SLOT_COUNT, STRUCT_SIZES, bind_casts, bind_fields, class, handle};
pub use rt::casts::NativeClass;
pub use generated::structs;
pub use generated::thunks::native_table;
pub use rt::registry::{Registry, Slot, SlotStatus, status, stub_calls};
pub use rt::strings::StrRet;
pub use rt::types::{Guid, HandleId, ManagedObject, NativeRef, PtrHandle, RawPtr, StringToken};
