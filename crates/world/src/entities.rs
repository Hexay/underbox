//! Entity lumps (`.vents_c`): a map's entities as key/value strings, plus child lumps (instanced
//! prefabs) by resource name. Entities come as KV3 (`keyValues3Data`) or, in older compiles, a
//! binary blob (`m_keyValuesData`) whose keys are only hashes. I/O connections and the
//! `attributes` set aren't kept: managed reads neither. Layout: research/world.md.

use crate::Error;
use crate::values::{array, field_str, number, text};
use kv::{Object, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Name(String),
    /// A legacy blob key: `StringToken` of the lowercased name (MurmurHash2, seed 0x31415926).
    Hash(u32),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entity {
    pub values: Vec<(Key, String)>,
}

impl Entity {
    /// A named value, matched case-blind (hashed keys can't be matched by name here).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.iter().find(|(k, _)| matches!(k, Key::Name(n) if n.eq_ignore_ascii_case(name))).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntityLump {
    pub name: String,
    /// Child lump resource names (`.vents`).
    pub children: Vec<String>,
    pub entities: Vec<Entity>,
}

fn kv3_entity(o: &Object) -> Result<Entity, Error> {
    let version = o.get("version").and_then(number).unwrap_or(0.0);
    if version != 1.0 {
        return Err(Error::Format(format!("entity data version {version}")));
    }
    let values = match o.get("values") {
        Some(Value::Object(values)) => values.0.iter().map(|(k, v)| (Key::Name(k.clone()), text(v))).collect(),
        _ => Vec::new(),
    };
    Ok(Entity { values })
}

struct Blob<'a>(&'a [u8]);

impl Blob<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let head = self.0.get(..N).ok_or_else(|| Error::Format("entity blob is truncated".into()))?;
        self.0 = &self.0[N..];
        Ok(head.try_into().unwrap())
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.take()?))
    }
    fn cstr(&mut self) -> Result<String, Error> {
        let end = self.0.iter().position(|&b| b == 0).ok_or_else(|| Error::Format("entity blob string is unterminated".into()))?;
        let s = String::from_utf8_lossy(&self.0[..end]).into_owned();
        self.0 = &self.0[end + 1..];
        Ok(s)
    }

    /// One typed value (`EntityFieldType`), as text.
    fn value(&mut self) -> Result<String, Error> {
        Ok(match self.u32()? {
            0x06 => self.take::<1>()?[0].min(1).to_string(),
            0x01 => self.f32()?.to_string(),
            0x22 => f64::from_le_bytes(self.take()?).to_string(),
            0x09 => self.take::<4>()?.map(|b| b.to_string()).join(" "),
            0x05 => i32::from_le_bytes(self.take()?).to_string(),
            0x25 => self.u32()?.to_string(),
            0x1A => i64::from_le_bytes(self.take()?).to_string(),
            0x21 => u64::from_le_bytes(self.take()?).to_string(),
            0x03 | 0x27 => [self.f32()?, self.f32()?, self.f32()?].map(|f| f.to_string()).join(" "),
            0x1E => self.cstr()?,
            t => return Err(Error::Format(format!("entity field type {t:#x}"))),
        })
    }
}

/// The legacy blob: version 1, value count, attribute count, then (hash, value) pairs and
/// (hash, name, value) attributes.
fn blob_entity(bytes: &[u8]) -> Result<Entity, Error> {
    let mut b = Blob(bytes);
    let version = b.u32()?;
    if version != 1 {
        return Err(Error::Format(format!("entity blob version {version}")));
    }
    let (count, attributes) = (b.u32()?, b.u32()?);
    let mut values = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let hash = b.u32()?;
        values.push((Key::Hash(hash), b.value()?));
    }
    for _ in 0..attributes {
        let (_hash, _name) = (b.u32()?, b.cstr()?);
        b.value()?;
    }
    Ok(Entity { values })
}

fn bytes(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::Binary(b) => Some(b.clone()),
        Value::Array(items) => items.iter().map(|i| number(i).map(|n| n as u8)).collect(),
        _ => None,
    }
}

/// An entity lump's decoded DATA.
pub fn parse(data: &Value) -> Result<EntityLump, Error> {
    let o = data.as_object().ok_or_else(|| Error::Format("entity lump DATA isn't an object".into()))?;
    let mut entities = Vec::new();
    for e in array(o, "m_entityKeyValues").iter().filter_map(Value::as_object) {
        let entity = match (e.get("keyValues3Data"), e.get("m_keyValuesData").and_then(bytes)) {
            (Some(Value::Object(kv3)), _) => kv3_entity(kv3)?,
            (_, Some(blob)) => blob_entity(&blob)?,
            _ => continue,
        };
        entities.push(entity);
    }
    let children = array(o, "m_childLumps").iter().filter_map(|v| v.as_str().map(str::to_owned)).filter(|s| !s.is_empty()).collect();
    Ok(EntityLump { name: field_str(o, "m_name"), children, entities })
}
