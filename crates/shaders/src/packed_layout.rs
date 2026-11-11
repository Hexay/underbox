//! Structured buffer element types: parsed from the source's struct definitions, sized tightly
//! (DX rules, what managed's `StructLayout` structs and native use) and under std430 (what
//! `-fvk-use-gl-layout` gives). See packed_buffers.rs.

use crate::scan::{ident_at, is_ident, match_close, occurrences, skip_trivia};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub enum Ty {
    /// A 32-bit scalar or vector; `base` is `uint`, `int`, `float` or `bool`.
    Vector { base: &'static str, n: u32 },
    /// `baseRxC`, stored column by column unless `row_major` (HLSL's default packing).
    Matrix { base: &'static str, r: u32, c: u32, row_major: bool },
    /// An `enum`: one `int`.
    Enum(String),
    Struct(String),
    Array(Box<Ty>, u32),
}

/// A leaf of a flattened type (a vector, matrix or enum): its access path (`.Pos`, `.a[1]`, or
/// empty) and first word.
pub struct Leaf {
    pub path: String,
    pub ty: Ty,
    pub word: u32,
}

pub struct Structs {
    /// Fields per struct name, or why the struct can't be laid out word by word.
    structs: HashMap<String, Result<Vec<(String, Ty)>, String>>,
    enums: HashSet<String>,
}

/// Each `struct Name { ... }`: its name and the span inside the braces.
pub fn struct_bodies(s: &str) -> impl Iterator<Item = (&str, (usize, usize))> {
    occurrences(s, "struct").filter_map(|i| {
        let at = skip_trivia(s, i + 6);
        let name = ident_at(s, at)?;
        let open = skip_trivia(s, at + name.len());
        (s.as_bytes().get(open) == Some(&b'{')).then_some(())?;
        Some((name, (open + 1, match_close(s, open, false)?)))
    })
}

impl Structs {
    pub fn parse(s: &str) -> Structs {
        let enums = occurrences(s, "enum")
            .filter_map(|i| {
                let mut at = skip_trivia(s, i + 4);
                if ident_at(s, at) == Some("class") {
                    at = skip_trivia(s, at + 5);
                }
                ident_at(s, at).map(str::to_string)
            })
            .collect();
        let row_major = s.contains("pack_matrix") && s.lines().any(|l| l.contains("pack_matrix") && l.contains("row_major"));
        let mut out = Structs { structs: HashMap::new(), enums };
        for (name, (start, end)) in struct_bodies(s) {
            out.structs.insert(name.to_string(), Err("defined in terms of itself".into()));
            let fields = out.fields_of(&strip_methods(&s[start..end]), row_major);
            out.structs.insert(name.to_string(), fields);
        }
        out
    }

    /// The type named `name`, if it can be an element: a known struct or enum, or a 32-bit
    /// scalar, vector or matrix.
    pub fn ty(&self, name: &str) -> Option<Ty> {
        self.named(name, false)
    }

    fn named(&self, name: &str, row_major: bool) -> Option<Ty> {
        if let Some(t) = vector(name).or_else(|| matrix(name, row_major)) {
            return Some(t);
        }
        if self.enums.contains(name) {
            return Some(Ty::Enum(name.to_string()));
        }
        self.structs.contains_key(name).then(|| Ty::Struct(name.to_string()))
    }

    fn fields(&self, name: &str) -> Result<&[(String, Ty)], String> {
        match self.structs.get(name) {
            Some(Ok(f)) => Ok(f),
            Some(Err(e)) => Err(format!("{name}: {e}")),
            None => Err(format!("{name}: unknown")),
        }
    }

    /// Size and alignment under std430.
    fn std430(&self, ty: &Ty) -> Result<(u32, u32), String> {
        let vec = |n: u32| (4 * n, [4, 8, 16, 16][n as usize - 1]);
        Ok(match ty {
            Ty::Vector { n, .. } => vec(*n),
            Ty::Enum(_) => vec(1),
            Ty::Matrix { r, c, row_major, .. } => {
                let (count, len) = if *row_major { (*r, *c) } else { (*c, *r) };
                let stride = vec(len).1;
                (count * stride, stride)
            }
            Ty::Array(t, len) => {
                let (size, align) = self.std430(t)?;
                (size.next_multiple_of(align) * len, align)
            }
            Ty::Struct(name) => {
                let (mut offset, mut align) = (0u32, 4);
                for (_, t) in self.fields(name)? {
                    let (s, a) = self.std430(t)?;
                    offset = offset.next_multiple_of(a) + s;
                    align = align.max(a);
                }
                (offset.next_multiple_of(align), align)
            }
        })
    }

    /// Size in 32-bit words, tightly packed.
    pub fn words(&self, ty: &Ty) -> Result<u32, String> {
        Ok(match ty {
            Ty::Vector { n, .. } => *n,
            Ty::Enum(_) => 1,
            Ty::Matrix { r, c, .. } => r * c,
            Ty::Array(t, len) => self.words(t)? * len,
            Ty::Struct(name) => self.fields(name)?.iter().map(|(_, t)| self.words(t)).sum::<Result<u32, String>>()?,
        })
    }

    /// Whether a buffer of `ty` has a different element stride or member offsets under std430
    /// than packed tightly. Tight packing is minimal, so equal strides mean equal offsets.
    pub fn padded(&self, ty: &Ty) -> Result<bool, String> {
        let (size, align) = self.std430(ty)?;
        Ok(size.next_multiple_of(align) != 4 * self.words(ty)?)
    }

    /// `ty`'s leaves in memory order, from word `word`, paths prefixed with `path`.
    pub fn leaves(&self, ty: &Ty, path: &str, word: u32, out: &mut Vec<Leaf>) -> Result<u32, String> {
        match ty {
            Ty::Array(t, len) => (0..*len).try_fold(word, |w, k| self.leaves(t, &format!("{path}[{k}]"), w, out)),
            Ty::Struct(name) => self.fields(name)?.iter().try_fold(word, |w, (f, t)| self.leaves(t, &format!("{path}.{f}"), w, out)),
            leaf => {
                out.push(Leaf { path: path.to_string(), ty: leaf.clone(), word });
                Ok(word + self.words(leaf)?)
            }
        }
    }

    /// A field of struct `name`: its type and first word.
    pub fn field(&self, name: &str, field: &str) -> Option<(Ty, u32)> {
        let mut word = 0;
        for (f, t) in self.fields(name).ok()? {
            if f == field {
                return Some((t.clone(), word));
            }
            word += self.words(t).ok()?;
        }
        None
    }

    /// A struct body's fields (methods already blanked).
    fn fields_of(&self, body: &str, row_major_default: bool) -> Result<Vec<(String, Ty)>, String> {
        if body.contains('#') {
            return Err("preprocessor lines in the body".into());
        }
        let mut out = Vec::new();
        for decl in body.split(';').map(str::trim).filter(|d| !d.is_empty()) {
            let decl = decl.split(':').next().unwrap_or(decl).trim();
            let mut row_major = row_major_default;
            let mut words = decl.split_whitespace().peekable();
            while let Some(&w) = words.peek() {
                match w {
                    "static" => break,
                    "row_major" => row_major = true,
                    "column_major" => row_major = false,
                    "precise" | "nointerpolation" | "linear" | "centroid" => {}
                    _ => break,
                }
                words.next();
            }
            if words.peek() == Some(&"static") {
                continue;
            }
            let ty_name = words.next().unwrap_or_default();
            let rest: String = words.collect::<Vec<_>>().join(" ");
            let ty = match self.named(ty_name, row_major) {
                Some(Ty::Struct(s)) => {
                    self.fields(&s)?;
                    Ty::Struct(s)
                }
                Some(t) => t,
                None => return Err(format!("field type `{ty_name}`")),
            };
            for name in rest.split(',').map(str::trim) {
                let (name, dims) = name.split_once('[').map_or((name, ""), |(n, d)| (n.trim(), d));
                if name.is_empty() || !name.bytes().all(is_ident) {
                    return Err(format!("field `{decl}`"));
                }
                let lens = dims.split('[').filter(|d| !d.is_empty()).map(|d| d.trim().trim_end_matches(']').trim().parse::<u32>());
                let lens: Vec<u32> = lens.collect::<Result<_, _>>().map_err(|_| format!("array size in `{decl}`"))?;
                out.push((name.to_string(), lens.iter().rev().fold(ty.clone(), |t, &len| Ty::Array(Box::new(t), len))));
            }
        }
        if out.is_empty() { Err("no fields".into()) } else { Ok(out) }
    }
}

/// HLSL's name for a leaf type (the parameter type of a store helper).
pub fn type_name(ty: &Ty) -> Option<String> {
    match ty {
        Ty::Vector { base, n: 1 } => Some(base.to_string()),
        Ty::Vector { base, n } => Some(format!("{base}{n}")),
        Ty::Matrix { base, r, c, .. } => Some(format!("{base}{r}x{c}")),
        Ty::Enum(name) | Ty::Struct(name) => Some(name.clone()),
        Ty::Array(..) => None,
    }
}

/// `body` with each member function (signature through closing brace) blanked.
fn strip_methods(body: &str) -> String {
    let mut out = body.to_string();
    let mut from = 0;
    while let Some(open) = out[from..].find('{').map(|o| from + o) {
        let start = out[..open].rfind([';', '}']).map_or(0, |i| i + 1);
        let Some(close) = match_close(&out, open, false) else { break };
        out.replace_range(start..=close, &" ".repeat(close + 1 - start));
        from = close + 1;
    }
    out
}

fn scalar(base: &str) -> Option<&'static str> {
    Some(match base {
        "float" | "half" => "float",
        "int" => "int",
        "uint" | "dword" => "uint",
        "bool" => "bool",
        _ => return None,
    })
}

/// A 32-bit scalar or vector type name (`float3`, `uint`, `half2`; 16-bit types are 32-bit
/// without `-enable-16bit-types`).
fn vector(name: &str) -> Option<Ty> {
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (base, n) = name.split_at(name.len() - digits);
    let n = if n.is_empty() { 1 } else { n.parse().ok().filter(|n| (1..=4).contains(n))? };
    Some(Ty::Vector { base: scalar(base)?, n })
}

/// `floatRxC` and the like.
fn matrix(name: &str, row_major: bool) -> Option<Ty> {
    let b = name.as_bytes();
    let (r, c) = (b.get(b.len().checked_sub(3)?)?, b.last()?);
    let dim = |d: &u8| (b'1'..=b'4').contains(d).then(|| (d - b'0') as u32);
    (b[b.len() - 2] == b'x').then_some(())?;
    Some(Ty::Matrix { base: scalar(&name[..name.len() - 3])?, r: dim(r)?, c: dim(c)?, row_major })
}
