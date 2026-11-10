//! The compiled resources a map holds keep their content as binary KV3 in the DATA block.

use crate::Error;
use assets::container::{Container, block};
use assets::kv3bin::{self, Compression};
use kv::Value;

/// A compiled resource's DATA block, decoded.
pub fn data(file: &[u8]) -> Result<Value, Error> {
    let payload = block(file, b"DATA").map_err(Error::Container)?.ok_or_else(|| Error::Format("no DATA block".into()))?;
    kv3bin::read(payload).map_err(Error::Kv3)
}

/// A compiled resource holding `data` (v5 binary KV3) after an empty reference list.
pub fn write(data: &Value, compression: Compression) -> Vec<u8> {
    let rerl = [8u32, 0].iter().flat_map(|w| w.to_le_bytes()).collect();
    Container { version: 0, blocks: vec![(*b"RERL", rerl), (*b"DATA", kv3bin::write(data, 5, compression))] }.write()
}
