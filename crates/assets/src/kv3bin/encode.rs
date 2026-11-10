//! Writing binary KV3 v4 or v5, laid out as ValveResourceFormat's serializer does. Used for test
//! fixtures (compiled worlds); integers go out as INT64, floats as DOUBLE, and arrays of 1..=255
//! doubles as byte-length typed arrays (how compiled vectors and matrices look).

use super::{LZ4, LZ4_FRAME, MAGIC_BASE, TRAILER, ZSTD};
use kv::Value;
use std::collections::HashMap;

/// KV3ID `generic`.
const FORMAT_GENERIC: [u8; 16] = [0x7C, 0x16, 0x12, 0x74, 0xE9, 0x06, 0x98, 0x46, 0xAF, 0xF2, 0xE6, 0x3E, 0xB5, 0x90, 0x37, 0xE7];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Lz4,
    Zstd,
}

impl Compression {
    fn method(self) -> u32 {
        match self {
            Compression::None => 0,
            Compression::Lz4 => LZ4,
            Compression::Zstd => ZSTD,
        }
    }

    fn pack(self, data: &[u8]) -> Vec<u8> {
        match self {
            Compression::None => data.to_vec(),
            Compression::Lz4 => lz4_flex::block::compress(data),
            Compression::Zstd => ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest),
        }
    }
}

#[derive(Default)]
struct Out {
    b1: Vec<u8>,
    b2: Vec<u8>,
    b4: Vec<u8>,
    b8: Vec<u8>,
    types: Vec<u8>,
    object_lengths: Vec<u8>,
    strings: Vec<String>,
    ids: HashMap<String, i32>,
    blobs: Vec<u8>,
    blob_lengths: Vec<usize>,
    objects: usize,
    arrays: usize,
}

fn flag_number(name: &str) -> u8 {
    ["resource", "resource_name", "panorama", "soundevent", "subclass"].iter().position(|n| *n == name).map_or(0, |i| i as u8 + 1)
}

impl Out {
    fn string(&mut self, s: &str) {
        let id = match self.ids.get(s) {
            Some(&id) => id,
            None => {
                self.strings.push(s.to_owned());
                let id = self.strings.len() as i32 - 1;
                self.ids.insert(s.to_owned(), id);
                id
            }
        };
        self.b4.extend_from_slice(&id.to_le_bytes());
    }

    fn ty(&mut self, t: u8, flag: u8) {
        if flag == 0 {
            self.types.push(t);
        } else {
            self.types.extend_from_slice(&[t | 0x80, flag]);
        }
    }

    fn value(&mut self, v: &Value, version: u32, flag: u8) {
        match v {
            Value::Flagged(name, inner) => self.value(inner, version, flag_number(name)),
            Value::Null => self.ty(1, flag),
            Value::Bool(b) => self.ty(if *b { 13 } else { 14 }, flag),
            Value::Int(0) => self.ty(15, flag),
            Value::Int(1) => self.ty(16, flag),
            Value::Int(i) => {
                self.ty(3, flag);
                self.b8.extend_from_slice(&i.to_le_bytes());
            }
            Value::Double(d) if d.to_bits() == 0 => self.ty(17, flag),
            Value::Double(d) if *d == 1.0 => self.ty(18, flag),
            Value::Double(d) => {
                self.ty(5, flag);
                self.b8.extend_from_slice(&d.to_le_bytes());
            }
            Value::String(s) => {
                self.ty(6, flag);
                self.string(s);
            }
            Value::Binary(b) => {
                self.ty(7, flag);
                self.blob_lengths.push(b.len());
                self.blobs.extend_from_slice(b);
            }
            Value::Array(items) => {
                self.arrays += 1;
                let doubles: Option<Vec<f64>> = items.iter().map(|i| if let Value::Double(d) = i { Some(*d) } else { None }).collect();
                match doubles.filter(|d| (1..=255).contains(&d.len())) {
                    Some(d) => {
                        self.ty(24, flag);
                        self.b1.push(d.len() as u8);
                        self.ty(5, 0);
                        d.iter().for_each(|x| self.b8.extend_from_slice(&x.to_le_bytes()));
                    }
                    None => {
                        self.ty(8, flag);
                        self.b4.extend_from_slice(&(items.len() as i32).to_le_bytes());
                        items.iter().for_each(|i| self.value(i, version, 0));
                    }
                }
            }
            Value::Object(o) => {
                self.objects += 1;
                self.ty(9, flag);
                let n = (o.0.len() as i32).to_le_bytes();
                if version >= 5 { self.object_lengths.extend_from_slice(&n) } else { self.b4.extend_from_slice(&n) }
                for (name, member) in &o.0 {
                    self.string(name);
                    self.value(member, version, 0);
                }
            }
        }
    }

    fn string_bytes(&self) -> Vec<u8> {
        self.strings.iter().flat_map(|s| s.bytes().chain([0])).collect()
    }

    /// Blobs as stored, plus the LZ4 frame sizes (LZ4 only).
    fn packed_blobs(&self, c: Compression) -> (Vec<u8>, Vec<u8>) {
        if c != Compression::Lz4 {
            return (c.pack(&self.blobs), Vec::new());
        }
        let (mut data, mut sizes, mut at) = (Vec::new(), Vec::new(), 0);
        for &len in &self.blob_lengths {
            for chunk in self.blobs[at..at + len].chunks(LZ4_FRAME) {
                let frame = lz4_flex::block::compress(chunk);
                sizes.extend_from_slice(&(frame.len() as u16).to_le_bytes());
                data.extend_from_slice(&frame);
            }
            at += len;
        }
        (data, sizes)
    }

    /// The blob lengths and trailer that end a buffer's types, then the LZ4 frame sizes.
    fn blob_table(&self, buf: &mut Vec<u8>, frame_sizes: &[u8]) {
        self.blob_lengths.iter().for_each(|&l| buf.extend_from_slice(&(l as i32).to_le_bytes()));
        buf.extend_from_slice(&TRAILER.to_le_bytes());
        buf.extend_from_slice(frame_sizes);
    }
}

fn lane(buf: &mut Vec<u8>, data: &[u8], align: usize, always: bool) {
    if !data.is_empty() || always {
        buf.resize(buf.len().next_multiple_of(align), 0);
    }
    buf.extend_from_slice(data);
}

fn put(out: &mut Vec<u8>, values: &[usize]) {
    values.iter().for_each(|&v| out.extend_from_slice(&(v as u32).to_le_bytes()));
}

/// Encode `value` as binary KV3 `version` (4 or 5).
pub fn write(value: &Value, version: u32, c: Compression) -> Vec<u8> {
    assert!(version == 4 || version == 5, "binary KV3 writes v4 or v5");
    let mut o = Out::default();
    o.value(value, version, 0);
    let (blobs, frames) = o.packed_blobs(c);
    let strings = o.string_bytes();
    let mut head = (MAGIC_BASE | version).to_le_bytes().to_vec();
    head.extend_from_slice(&FORMAT_GENERIC);
    put(&mut head, &[c.method() as usize]);
    head.extend_from_slice(&0u16.to_le_bytes());
    head.extend_from_slice(&(if c == Compression::Lz4 { LZ4_FRAME as u16 } else { 0 }).to_le_bytes());
    let hints = (o.objects.min(0xFFFF) as u32) | ((o.arrays.min(0xFFFF) as u32) << 16);
    let zstd_blobs = if c == Compression::Zstd { blobs.len() } else { 0 };
    let mut body;
    if version == 5 {
        let mut buf1 = strings.clone();
        lane(&mut buf1, &(o.strings.len() as i32).to_le_bytes(), 4, true);
        let mut buf2 = o.object_lengths.clone();
        lane(&mut buf2, &o.b1, 1, false);
        lane(&mut buf2, &o.b2, 2, false);
        lane(&mut buf2, &o.b4, 4, false);
        lane(&mut buf2, &o.b8, 8, false);
        buf2.extend_from_slice(&o.types);
        o.blob_table(&mut buf2, &frames);
        let (c1, c2) = (c.pack(&buf1), c.pack(&buf2));
        let packed = |n: usize| if c == Compression::None { 0 } else { n };
        put(&mut head, &[strings.len(), 1, 0, o.types.len()]);
        head.extend_from_slice(&hints.to_le_bytes());
        put(&mut head, &[buf1.len() + buf2.len(), c1.len() + c2.len() + zstd_blobs, o.blob_lengths.len(), o.blobs.len(), 0, frames.len()]);
        put(&mut head, &[buf1.len(), packed(c1.len()), buf2.len(), packed(c2.len())]);
        put(&mut head, &[o.b1.len(), o.b2.len() / 2, o.b4.len() / 4, o.b8.len() / 8, 0, o.objects, o.arrays, 0]);
        body = [c1, c2].concat();
    } else {
        let mut b4 = (o.strings.len() as i32).to_le_bytes().to_vec();
        b4.extend_from_slice(&o.b4);
        let mut buf = o.b1.clone();
        lane(&mut buf, &o.b2, 2, false);
        lane(&mut buf, &b4, 4, false);
        lane(&mut buf, &o.b8, 8, true);
        buf.extend_from_slice(&strings);
        buf.extend_from_slice(&o.types);
        o.blob_table(&mut buf, &frames);
        let packed = c.pack(&buf);
        put(&mut head, &[o.b1.len(), b4.len() / 4, o.b8.len() / 8, strings.len() + o.types.len()]);
        head.extend_from_slice(&hints.to_le_bytes());
        put(&mut head, &[buf.len(), packed.len() + zstd_blobs, o.blob_lengths.len(), o.blobs.len(), o.b2.len() / 2, 0]);
        body = packed;
    }
    if !o.blob_lengths.is_empty() {
        body.extend_from_slice(&blobs);
        body.extend_from_slice(&TRAILER.to_le_bytes());
    }
    head.extend_from_slice(&body);
    head
}
