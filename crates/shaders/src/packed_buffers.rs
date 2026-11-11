//! `StructuredBuffer<T>` / `RWStructuredBuffer<T>` whose element std430 would pad → a `uint` word
//! buffer read and written leaf by leaf, packed tightly.
//!
//! Managed fills structured buffers from sequential C# structs and native compiles them with
//! DX layout: 4-byte alignment, no padding (`SpriteData.Rotation` at 12, stride 148).
//! `-fvk-use-gl-layout` (dxc.rs) gives them std430 instead: a `float3` aligns to 16 and strides
//! round up (`Rotation` at 16, stride 176), so every later field reads the wrong bytes;
//! sprite_cs then took `Splots` from a float and looped for billions of iterations. DXC's
//! scalar layout would change cbuffers too, and templated `ByteAddressBuffer.Load<T>` follows the
//! same std430 rule, so the words are read explicitly. Elements std430 leaves unpadded keep
//! their type. Rewrites, all on the declaration's line or in place so line numbers hold:
//! - `B[i]`, `B.Load( i )` → `B_UbLoad( i )`; `B[i] = v;` → `B_UbStore( i, v );`
//! - `B[i].f = v;` → `B_UbStore_f( i, v );` (reads `B[i].f` stay `B_UbLoad( i ).f`)
//! - `B.GetDimensions( n, s )` → `B_UbDims( n, s )`.
//!
//! A buffer used any other way (compound or swizzled writes, atomics, passed whole, a member
//! type that isn't a 32-bit scalar, vector, array or struct of them) is left as it was;
//! [`audit`] says why.

use crate::packed_layout::{Leaf, Structs, Ty, struct_bodies, type_name};
use crate::scan::{blank_comments, buffer_declaration, ident_at, is_assign, is_compound_assign, match_close, occurrences, skip_trivia, statement_end};

const KEYWORDS: [&str; 2] = ["StructuredBuffer", "RWStructuredBuffer"];

pub fn rewrite(src: &str) -> String {
    rewrite_reporting(src).0
}

/// Each padded buffer as `name<T>: rewritten` or `name<T>: kept (reason)`.
pub fn audit(src: &str) -> Vec<String> {
    rewrite_reporting(src).1
}

fn rewrite_reporting(src: &str) -> (String, Vec<String>) {
    let clean = blank_comments(src);
    let structs = Structs::parse(&clean);
    let mut out = src.to_string();
    let mut report = Vec::new();
    for (keyword, ty_name, buffer) in padded_buffers(&clean, &structs, &mut report) {
        let ty = structs.ty(&ty_name).expect("padded_buffers checked the type");
        match rewrite_buffer(&out, keyword, &buffer, &ty_name, &ty, &structs) {
            Ok(r) => {
                out = r;
                report.push(format!("{buffer}<{ty_name}>: rewritten"));
            }
            Err(e) => report.push(format!("{buffer}<{ty_name}>: kept ({e})")),
        }
    }
    (out, report)
}

/// `(keyword, element type, buffer)` for each global declaration whose element std430 pads.
fn padded_buffers(s: &str, structs: &Structs, report: &mut Vec<String>) -> Vec<(&'static str, String, String)> {
    let mut out: Vec<(&'static str, String, String)> = Vec::new();
    for keyword in KEYWORDS {
        for i in occurrences(s, keyword) {
            let Some((ty_name, name, (_, ty_end))) = buffer_declaration(s, i, keyword) else { continue };
            let after = skip_trivia(s, name_at(s, ty_end) + name.len());
            let global = matches!(s.as_bytes().get(after), Some(b';' | b':' | b'<'));
            let Some(ty) = structs.ty(ty_name) else { continue };
            if !global || out.iter().any(|(_, _, b)| b == name) {
                continue;
            }
            match structs.padded(&ty) {
                Ok(true) => out.push((keyword, ty_name.to_string(), name.to_string())),
                Ok(false) => {}
                Err(e) => report.push(format!("{name}<{ty_name}>: kept ({e})")),
            }
        }
    }
    out
}

/// What a buffer's uses need besides `_UbLoad`.
#[derive(Default)]
struct Needs {
    store: bool,
    dims: bool,
    fields: Vec<String>,
}

fn rewrite_buffer(src: &str, keyword: &str, buffer: &str, ty_name: &str, ty: &Ty, structs: &Structs) -> Result<String, String> {
    let s = blank_comments(src);
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut needs = Needs::default();
    let mut decl_end = None;
    let fields = field_spans(&s);
    // A member of the same name (`x.B`, or a struct field) isn't the buffer; uses in a struct's
    // member functions are.
    let member = |i: usize| s[..i].trim_end().ends_with('.') || fields.iter().any(|&(a, b)| a <= i && i < b);
    for i in occurrences(&s, buffer).filter(|&i| !member(i)) {
        if inside_atomic(&s, i) {
            return Err("an atomic on it".into());
        }
        let next = skip_trivia(&s, i + buffer.len());
        match s.as_bytes().get(next) {
            Some(b'[') => {
                let close = match_close(&s, next, false).ok_or("unbalanced index")?;
                let index = src[next + 1..close].trim();
                if occurrences(index, buffer).next().is_some() {
                    return Err("indexed by itself".into());
                }
                let (field, chain_end) = postfix_chain(&s, close + 1);
                let after = skip_trivia(&s, chain_end);
                if is_compound_assign(&s, after) {
                    return Err("a compound assignment".into());
                }
                if !is_assign(&s, after) {
                    edits.push((i, close + 1, format!("{buffer}_UbLoad( {index} )")));
                    continue;
                }
                let semi = statement_end(&s, after + 1).ok_or("an assignment outside a statement")?;
                let value = src[after + 1..semi].trim();
                let field = match field {
                    _ if chain_end == close + 1 => None,
                    Some((f, end)) if end == chain_end => Some(f),
                    _ => return Err("a write to part of a field".into()),
                };
                match field {
                    None => {
                        needs.store = true;
                        edits.push((i, semi, format!("{buffer}_UbStore( {index}, {value} )")));
                    }
                    Some(f) => {
                        let Ty::Struct(name) = ty else { return Err("a field write on a vector".into()) };
                        structs.field(name, &f).ok_or_else(|| format!("unknown field {f}"))?;
                        edits.push((i, semi, format!("{buffer}_UbStore_{f}( {index}, {value} )")));
                        needs.fields.push(f);
                    }
                }
            }
            Some(b'.') => {
                let at = skip_trivia(&s, next + 1);
                let method = ident_at(&s, at).unwrap_or_default();
                let helper = match method {
                    "Load" => "UbLoad",
                    "GetDimensions" => {
                        needs.dims = true;
                        "UbDims"
                    }
                    _ => return Err(format!("method {method}")),
                };
                edits.push((i, at + method.len(), format!("{buffer}_{helper}")));
            }
            _ if decl_end.is_none() && is_declaration(&s, i, keyword, buffer) => {
                let start = s[..i].rfind(keyword).ok_or("no declaration")?;
                let (_, _, (ty_start, ty_end)) = buffer_declaration(&s, start, keyword).ok_or("no declaration")?;
                edits.push((ty_start, ty_end, "uint".into()));
                decl_end = Some(statement_end(&s, next).ok_or("declaration without `;`")?);
            }
            _ => return Err("used whole (passed or aliased)".into()),
        }
    }
    let semi = decl_end.ok_or("no global declaration")?;
    needs.fields.sort();
    needs.fields.dedup();
    edits.push((semi + 1, semi + 1, helpers(buffer, ty_name, ty, structs, &needs)?));
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = src.to_string();
    for (start, end, text) in edits {
        out.replace_range(start..end, &text);
    }
    Ok(out)
}

/// The parts of struct bodies outside their member functions' braces.
fn field_spans(s: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (_, (mut a, end)) in struct_bodies(s) {
        while let Some(open) = s[a..end].find('{').map(|o| a + o) {
            out.push((a, open));
            a = match_close(s, open, false).map_or(end, |c| c + 1);
        }
        out.push((a, end));
    }
    out
}

/// The buffer named at `i` is the one declared right before it.
fn is_declaration(s: &str, i: usize, keyword: &str, buffer: &str) -> bool {
    s[..i].rfind(keyword).and_then(|d| buffer_declaration(s, d, keyword)).is_some_and(|(_, b, (_, ty_end))| b == buffer && name_at(s, ty_end) == i)
}

/// Where the declared name starts, given the end of `<T`.
fn name_at(s: &str, ty_end: usize) -> usize {
    skip_trivia(s, skip_trivia(s, ty_end) + 1)
}

/// The `.a[1].b`... chain from `i`: its first member with that member's end, and the chain's end.
fn postfix_chain(s: &str, i: usize) -> (Option<(String, usize)>, usize) {
    let (mut first, mut end) = (None, i);
    loop {
        let at = skip_trivia(s, end);
        match s.as_bytes().get(at) {
            Some(b'.') => {
                let m = skip_trivia(s, at + 1);
                let Some(name) = ident_at(s, m) else { return (first, end) };
                if end == i {
                    first = Some((name.to_string(), m + name.len()));
                }
                end = m + name.len();
            }
            Some(b'[') => match match_close(s, at, false) {
                Some(c) => end = c + 1,
                None => return (first, end),
            },
            _ => return (first, end),
        }
    }
}

/// Whether `i` sits in the argument list of an `Interlocked*` call.
fn inside_atomic(s: &str, i: usize) -> bool {
    s.match_indices("Interlocked").any(|(j, _)| {
        let word = ident_at(s, j).unwrap_or_default();
        let open = skip_trivia(s, j + word.len());
        j < i && s.as_bytes().get(open) == Some(&b'(') && match_close(s, open, false).is_some_and(|c| open < i && i < c)
    })
}

fn helpers(buffer: &str, ty_name: &str, ty: &Ty, structs: &Structs, needs: &Needs) -> Result<String, String> {
    let n = structs.words(ty)?;
    let mut leaves = Vec::new();
    structs.leaves(ty, "", 0, &mut leaves)?;
    let head = |ret: &str, name: &str, params: &str| format!(" {ret} {buffer}_{name}( uint ub_i{params} ) {{ uint ub_w = ub_i * {n};");
    let loads: String = leaves.iter().map(|l| format!(" ub_s{} = {};", l.path, load(buffer, l))).collect();
    let mut out = format!("{} {ty_name} ub_s;{loads} return ub_s; }}", head(ty_name, "UbLoad", ""));
    if needs.store {
        out += &format!("{}{} }}", head("void", "UbStore", &format!(", {ty_name} ub_s")), stores(buffer, "ub_s", &leaves));
    }
    for f in &needs.fields {
        let Ty::Struct(name) = ty else { unreachable!("field writes need a struct") };
        let (fty, word) = structs.field(name, f).ok_or_else(|| format!("unknown field {f}"))?;
        let fname = type_name(&fty).ok_or_else(|| format!("a write to array field {f}"))?;
        let mut leaves = Vec::new();
        structs.leaves(&fty, "", word, &mut leaves)?;
        out += &format!("{}{} }}", head("void", &format!("UbStore_{f}"), &format!(", {fname} ub_v")), stores(buffer, "ub_v", &leaves));
    }
    if needs.dims {
        out += &format!(" void {buffer}_UbDims( out uint ub_n, out uint ub_stride ) {{ {buffer}.GetDimensions( ub_n, ub_stride ); ub_n /= {n}; ub_stride = {}; }}", 4 * n);
    }
    Ok(out)
}

/// Word `w` of the element as `base` (`uint`, `int`, `float` or `bool`), vectorised over `words`.
fn from_bits(buffer: &str, base: &str, words: &[u32]) -> String {
    let refs: Vec<String> = words.iter().map(|w| format!("{buffer}[ub_w + {w}]")).collect();
    let raw = if refs.len() == 1 { refs[0].clone() } else { format!("uint{}( {} )", refs.len(), refs.join(", ")) };
    match base {
        "float" => format!("asfloat( {raw} )"),
        "int" => format!("asint( {raw} )"),
        "bool" => format!("( {raw} != 0 )"),
        _ => raw,
    }
}

fn to_bits(base: &str, value: &str) -> String {
    match base {
        "float" | "int" => format!("asuint( {value} )"),
        "bool" => format!("(uint)( {value} )"),
        _ => format!("( {value} )"),
    }
}

/// `(row, column, word)` of each matrix element, in HLSL constructor order.
fn matrix_words(r: u32, c: u32, row_major: bool, first: u32) -> impl Iterator<Item = (u32, u32, u32)> {
    (0..r).flat_map(move |i| (0..c).map(move |j| (i, j, first + if row_major { i * c + j } else { j * r + i })))
}

fn load(buffer: &str, l: &Leaf) -> String {
    match &l.ty {
        Ty::Vector { base, n } => from_bits(buffer, base, &(l.word..l.word + n).collect::<Vec<_>>()),
        Ty::Enum(name) => format!("({name})asint( {buffer}[ub_w + {}] )", l.word),
        Ty::Matrix { base, r, c, row_major } => {
            let elems: Vec<String> = matrix_words(*r, *c, *row_major, l.word).map(|(_, _, w)| from_bits(buffer, base, &[w])).collect();
            format!("{base}{r}x{c}( {} )", elems.join(", "))
        }
        Ty::Struct(_) | Ty::Array(..) => unreachable!("leaves are vectors, matrices or enums"),
    }
}

fn stores(buffer: &str, var: &str, leaves: &[Leaf]) -> String {
    let mut out = String::new();
    let mut store = |w: u32, bits: String| out += &format!(" {buffer}[ub_w + {w}] = {bits};");
    for l in leaves {
        let v = format!("{var}{}", l.path);
        match &l.ty {
            Ty::Vector { base, n: 1 } => store(l.word, to_bits(base, &v)),
            Ty::Vector { base, n } => (0..*n).for_each(|k| store(l.word + k, to_bits(base, &format!("{v}.{}", ["x", "y", "z", "w"][k as usize])))),
            Ty::Enum(_) => store(l.word, format!("asuint( (int)( {v} ) )")),
            Ty::Matrix { base, r, c, row_major } => matrix_words(*r, *c, *row_major, l.word).for_each(|(i, j, w)| store(w, to_bits(base, &format!("{v}[{i}][{j}]")))),
            Ty::Struct(_) | Ty::Array(..) => unreachable!("leaves are vectors, matrices or enums"),
        }
    }
    out
}

#[cfg(test)]
#[path = "packed_buffers_tests.rs"]
mod tests;
