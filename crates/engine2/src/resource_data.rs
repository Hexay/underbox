//! The asset loader's request objects (assetloader.def): `IAsyncResourceDataRequest` (a file read
//! for a resource) and `IRD_RegisterResourceDataUtils` (what a data registration callback reports
//! back). Native hands them to its resource-type callbacks; nothing in sbox-public calls these
//! slots and no callback of ours passes one yet, so these are the objects for a loader that does.

use abi::slots::{i_async_resource_data_request as req, ird_register_resource_data_utils as utils};
use abi::{NativeRef, RawPtr, Registry, StrRet, bind, class};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct DataRequest {
    pub file_name: String,
    /// Fixed once made, so `GetResultBuffer` stays valid for the request's life.
    pub buffer: Box<[u8]>,
}

/// A finished read of `file_name`.
pub fn data_request(file_name: &str, bytes: Vec<u8>) -> NativeRef<class::IAsyncResourceDataRequest> {
    objects::create(DataRequest { file_name: file_name.into(), buffer: bytes.into_boxed_slice() })
}

pub struct RegisterUtils {
    pub reloading: bool,
    pub result_size: i64,
    failed: AtomicBool,
    final_data: AtomicUsize,
}

/// The utils a registration callback gets: whether it's a reload, and the loaded data's size.
pub fn register_utils(reloading: bool, result_size: i64) -> NativeRef<class::IRD_RegisterResourceDataUtils> {
    objects::create(RegisterUtils { reloading, result_size, failed: AtomicBool::new(false), final_data: AtomicUsize::new(0) })
}

fn with_utils<R: Default>(u: NativeRef<class::IRD_RegisterResourceDataUtils>, f: impl FnOnce(&RegisterUtils) -> R) -> R {
    objects::get::<_, RegisterUtils>(u).map(|u| f(&u)).unwrap_or_default()
}

fn with_request<R: Default>(r: NativeRef<class::IAsyncResourceDataRequest>, f: impl FnOnce(&DataRequest) -> R) -> R {
    objects::get::<_, DataRequest>(r).map(|r| f(&r)).unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    bind!(r, req::GET_FILE_NAME, |q| StrRet::from(with_request(q, |q| Some(q.file_name.clone()))));
    bind!(r, req::GET_RESULT_BUFFER, |q| {
        with_request(q, |q| Some(RawPtr::from_raw(q.buffer.as_ptr().cast_mut().cast::<c_void>()))).unwrap_or(RawPtr::NULL)
    });
    bind!(r, req::GET_RESULT_BUFFER_SIZE, |q| with_request(q, |q| q.buffer.len() as i64));

    bind!(r, utils::IS_RELOADING, |u| with_utils(u, |u| u.reloading));
    bind!(r, utils::GET_RESULT_BUFFER_SIZE, |u| with_utils(u, |u| u.result_size));
    bind!(r, utils::SET_DATA_REGISTRATION_FAILED, |u| with_utils(u, |u| u.failed.store(true, Ordering::Relaxed)));
    bind!(r, utils::GET_DATA_REGISTRATION_FAILED, |u| with_utils(u, |u| u.failed.load(Ordering::Relaxed)));
    bind!(r, utils::SET_FINAL_RESOURCE_DATA, |u, p: RawPtr<c_void>| with_utils(u, |u| u.final_data.store(p.as_ptr() as usize, Ordering::Relaxed)));
    bind!(r, utils::GET_FINAL_RESOURCE_DATA, |u| RawPtr::from_raw(with_utils(u, |u| u.final_data.load(Ordering::Relaxed)) as *mut c_void));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_hold_their_file() {
        let q = data_request("models/a.vmdl_c", vec![1, 2, 3]);
        assert_eq!(with_request(q, |q| (q.file_name.clone(), q.buffer.to_vec())), ("models/a.vmdl_c".into(), vec![1, 2, 3]));
        objects::destroy(q);
        assert_eq!(with_request(q, |q| q.buffer.len()), 0, "a dead request reads as empty");
    }

    #[test]
    fn utils_record_what_the_callback_reports() {
        let u = register_utils(true, 64);
        assert!(with_utils(u, |u| u.reloading && u.result_size == 64));
        with_utils(u, |u| u.failed.store(true, Ordering::Relaxed));
        with_utils(u, |u| u.final_data.store(0x1234, Ordering::Relaxed));
        assert_eq!(with_utils(u, |u| (u.failed.load(Ordering::Relaxed), u.final_data.load(Ordering::Relaxed))), (true, 0x1234));
        objects::destroy(u);
    }
}
