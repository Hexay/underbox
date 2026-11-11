//! Feature declarations from a native-compiled FEATURE body. Native's embedded HLSL blanks the
//! FEATURES block, so a `StaticCombo( S_X, F_X, ... )` there only resolves against these records.
//!
//! A record (checked on Sandbox and Grubs files, VCS 65 and 68): a 64-byte name, a 64-byte group,
//! then i32s whose max sits at +136 (VCS 65) or +140 (VCS 68). Min is assumed to be the i32 before
//! max; every real record has min 0, so that field is unconfirmed.

const FIELD: usize = 64;

#[derive(Debug, PartialEq)]
pub struct NativeFeature {
    pub name: String,
    pub group: String,
    pub min: i32,
    pub max: i32,
}

fn cstr(b: &[u8]) -> Option<&str> {
    std::str::from_utf8(&b[..b.iter().position(|c| *c == 0)?]).ok()
}

fn i32_at(b: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn record(body: &[u8], at: usize, max_at: usize) -> Option<NativeFeature> {
    let name = cstr(body.get(at..at + FIELD)?)?;
    let group = cstr(body.get(at + FIELD..at + 2 * FIELD)?)?;
    let valid = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_graphic() || c == b' ');
    if !valid(name) || !valid(group) || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
        return None;
    }
    let (min, max) = (i32_at(body, at + max_at - 4)?, i32_at(body, at + max_at)?);
    (min >= 0 && (min..=255).contains(&max)).then(|| NativeFeature { name: name.into(), group: group.into(), min, max })
}

/// Every feature record in `body` (a whole VCS program body, magic first).
pub fn read(body: &[u8]) -> Vec<NativeFeature> {
    let max_at = match i32_at(body, 4) {
        Some(v) if v >= 68 => 140,
        _ => 136,
    };
    let mut out: Vec<NativeFeature> = Vec::new();
    let mut at = 1;
    while at < body.len() {
        if body[at - 1] == 0 && body[at..].starts_with(b"F_")
            && let Some(f) = record(body, at, max_at)
            && !out.iter().any(|o| o.name == f.name)
        {
            at += 2 * FIELD;
            out.push(f);
            continue;
        }
        at += 1;
    }
    out
}

/// A FEATURES block declaring `features`, for source whose own block native blanked.
pub fn block(features: &[NativeFeature]) -> String {
    let decls: String = features.iter().map(|f| format!("Feature( {}, {}..{}, \"{}\" );\n", f.name, f.min, f.max, f.group)).collect();
    format!("FEATURES\n{{\n{decls}}}\n")
}
