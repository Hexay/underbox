//! Binary KV3 values: types come from the type stream in depth-first order, and each value takes
//! its payload from the lane of its width (research/resource-container.md §4.4).

use super::{Cursor, Error, LZ4_FRAME};
use kv::{Object, Value};

#[derive(Clone, Copy)]
pub(super) struct Lanes<'a> {
    pub b1: &'a [u8],
    pub b2: &'a [u8],
    pub b4: &'a [u8],
    pub b8: &'a [u8],
}

fn take<const N: usize>(lane: &mut &[u8]) -> Result<[u8; N], Error> {
    if lane.len() < N {
        return Err(Error::Truncated);
    }
    let (head, rest) = lane.split_at(N);
    *lane = rest;
    Ok(head.try_into().unwrap())
}

pub(super) struct Ctx<'a> {
    pub version: u32,
    pub types: &'a [u8],
    /// v5: every object's member count, in depth-first order.
    pub object_lengths: &'a [u8],
    pub strings: Vec<String>,
    pub blobs: std::vec::IntoIter<Vec<u8>>,
    pub main: Lanes<'a>,
    /// v5's buffer 1, which `ARRAY_TYPE_AUXILIARY_BUFFER` elements read from; before v5 that's `main`.
    pub aux: Option<Lanes<'a>>,
}

/// The text name a KV3 flag byte stands for (ValveKeyValue's `KVFlag`).
fn flag_name(flag: u8) -> String {
    match flag {
        1 => "resource".into(),
        2 => "resource_name".into(),
        3 => "panorama".into(),
        4 => "soundevent".into(),
        5 => "subclass".into(),
        n => format!("flag{n}"),
    }
}

/// v2 stored flags as bits; only the lowest one counts (bit 4, multi-line strings, is dropped).
fn legacy_flag(bits: u8) -> u8 {
    [(1, 1), (2, 2), (8, 3), (16, 4), (32, 5)].into_iter().find(|(bit, _)| bits & bit != 0).map_or(0, |(_, f)| f)
}

impl<'a> Ctx<'a> {
    fn byte(&mut self) -> Result<u8, Error> {
        Ok(take::<1>(&mut self.types)?[0])
    }

    fn ty(&mut self) -> Result<(u8, u8), Error> {
        let b = self.byte()?;
        if self.version < 3 {
            let flag = if b & 0x80 != 0 { legacy_flag(self.byte()?) } else { 0 };
            return Ok((b & 0x7F, flag));
        }
        let flag = if b & 0x80 != 0 { self.byte()? } else { 0 };
        if b & 0x40 != 0 {
            self.byte()?;
        }
        Ok((b & 0x3F, flag))
    }

    pub fn root(&mut self) -> Result<Value, Error> {
        let (t, flag) = self.ty()?;
        self.flagged(t, flag, false)
    }

    fn flagged(&mut self, t: u8, flag: u8, aux: bool) -> Result<Value, Error> {
        let v = self.value(t, aux)?;
        Ok(if flag == 0 { v } else { Value::Flagged(flag_name(flag), Box::new(v)) })
    }

    fn lane(&mut self, aux: bool) -> &mut Lanes<'a> {
        match (&mut self.aux, aux) {
            (Some(a), true) => a,
            _ => &mut self.main,
        }
    }

    fn int(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(take(&mut self.main.b4)?))
    }

    fn count(&mut self) -> Result<usize, Error> {
        usize::try_from(self.int()?).map_err(|_| Error::Truncated)
    }

    fn string(&mut self) -> Result<String, Error> {
        let id = self.int()?;
        // -1 (and any id out of range) is the empty string.
        Ok(usize::try_from(id).ok().and_then(|i| self.strings.get(i)).cloned().unwrap_or_default())
    }

    fn value(&mut self, t: u8, aux: bool) -> Result<Value, Error> {
        Ok(match t {
            1 => Value::Null,
            13 => Value::Bool(true),
            14 => Value::Bool(false),
            15 | 26 => Value::Int(0),
            16 | 27 => Value::Int(1),
            17 => Value::Double(0.0),
            18 => Value::Double(1.0),
            2 => Value::Bool(take::<1>(&mut self.lane(aux).b1)?[0] != 0),
            22 => Value::Int(i8::from_le_bytes(take(&mut self.lane(aux).b1)?).into()),
            23 => Value::Int(u8::from_le_bytes(take(&mut self.lane(aux).b1)?).into()),
            20 => Value::Int(i16::from_le_bytes(take(&mut self.lane(aux).b2)?).into()),
            21 => Value::Int(u16::from_le_bytes(take(&mut self.lane(aux).b2)?).into()),
            11 => Value::Int(i32::from_le_bytes(take(&mut self.lane(aux).b4)?).into()),
            12 => Value::Int(u32::from_le_bytes(take(&mut self.lane(aux).b4)?).into()),
            19 => Value::Double(f32::from_le_bytes(take(&mut self.lane(aux).b4)?).into()),
            3 => Value::Int(i64::from_le_bytes(take(&mut self.lane(aux).b8)?)),
            4 => Value::Int(u64::from_le_bytes(take(&mut self.lane(aux).b8)?) as i64),
            5 => Value::Double(f64::from_le_bytes(take(&mut self.lane(aux).b8)?)),
            6 => Value::String(self.string()?),
            7 => Value::Binary(self.blobs.next().ok_or(Error::Truncated)?),
            8 => {
                let n = self.count()?;
                Value::Array((0..n).map(|_| self.root()).collect::<Result<_, _>>()?)
            }
            10 | 24 | 25 => {
                let n = if t == 10 { self.count()? } else { take::<1>(&mut self.main.b1)?[0].into() };
                let (element, flag) = self.ty()?;
                Value::Array((0..n).map(|_| self.flagged(element, flag, t == 25)).collect::<Result<_, _>>()?)
            }
            9 => {
                let n = if self.version >= 5 { usize::try_from(i32::from_le_bytes(take(&mut self.object_lengths)?)).map_err(|_| Error::Truncated)? } else { self.count()? };
                let mut members = Vec::with_capacity(n);
                for _ in 0..n {
                    let (t, flag) = self.ty()?;
                    let name = self.string()?;
                    members.push((name, self.flagged(t, flag, false)?));
                }
                Value::Object(Object(members))
            }
            other => return Err(Error::Type(other)),
        })
    }
}

/// LZ4 blob data: frames of at most [`LZ4_FRAME`] decoded bytes that never span two blobs, sized
/// by `frames`, each chained to everything decoded before it (a 64 KiB window).
pub(super) fn lz4_frames(c: &mut Cursor, frames: &mut Cursor, total: usize, lengths: &[usize]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(total);
    for &len in lengths {
        let mut left = len;
        while left > 0 {
            let size = usize::from(u16::from_le_bytes(frames.take(2)?.try_into().unwrap()));
            let want = left.min(LZ4_FRAME);
            let dict = &out[out.len().saturating_sub(65536)..];
            let frame = lz4_flex::block::decompress_with_dict(c.take(size)?, want, dict).map_err(|e| Error::Decompress(e.to_string()))?;
            if frame.is_empty() || frame.len() > left {
                return Err(Error::Decompress(format!("LZ4 blob frame of {} bytes", frame.len())));
            }
            left -= frame.len();
            out.extend_from_slice(&frame);
        }
    }
    Ok(out)
}
