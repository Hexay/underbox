//! `REDI` (resource edit info): what a resource was compiled from and by which compiler.
//! Layout: research/resource-container.md §3.2. We write the input and special dependencies;
//! the other eight lists stay empty.

/// One `{u32 rel offset, u32 count}` per list, ten lists.
const LISTS: usize = 10;
const INPUT: usize = 0;
const SPECIAL: usize = 3;
/// `InputDependency.flags`: the file existed at compile time.
const FILE_EXISTS: u32 = 2;

pub struct EditInfo<'a> {
    /// Source path relative to its content root, e.g. `scenes/minimal.scene`.
    pub input: Option<(&'a str, u32)>,
    /// (string, compiler identifier, fingerprint), e.g. Facepunch's managed compiler marker.
    pub special: Vec<(&'a str, &'a str, u32)>,
}

/// Each element is 16 bytes: two rel-offset strings then two u32s. Strings follow all elements,
/// and an offset counts from its own field.
pub fn write(info: &EditInfo) -> Vec<u8> {
    let elements: Vec<(usize, [&str; 2], [u32; 2])> = info
        .input
        .iter()
        .map(|&(path, crc)| (INPUT, [path, "game"], [crc, FILE_EXISTS]))
        .chain(info.special.iter().map(|&(s, compiler, fp)| (SPECIAL, [s, compiler], [fp, 0])))
        .collect();
    let mut out = vec![0u8; LISTS * 8 + elements.len() * 16];
    let mut fixups = Vec::new();
    for list in 0..LISTS {
        let first = elements.iter().position(|e| e.0 == list);
        let count = elements.iter().filter(|e| e.0 == list).count() as u32;
        if let Some(first) = first {
            put(&mut out, list * 8, (LISTS * 8 + first * 16 - list * 8) as u32);
        }
        put(&mut out, list * 8 + 4, count);
    }
    for (i, (_, strings, ints)) in elements.iter().enumerate() {
        let at = LISTS * 8 + i * 16;
        fixups.push((at, strings[0]));
        fixups.push((at + 4, strings[1]));
        put(&mut out, at + 8, ints[0]);
        put(&mut out, at + 12, ints[1]);
    }
    for (field, s) in fixups {
        let rel = (out.len() - field) as u32;
        put(&mut out, field, rel);
        out.extend_from_slice(s.as_bytes());
        out.push(0);
    }
    out
}

fn put(out: &mut [u8], at: usize, v: u32) {
    out[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(b: &[u8], at: usize) -> usize {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
    }

    /// The NUL-terminated string a rel-offset field at `at` points to.
    fn string_at(b: &[u8], at: usize) -> &str {
        let start = at + u32_at(b, at);
        let end = start + b[start..].iter().position(|&c| c == 0).unwrap();
        std::str::from_utf8(&b[start..end]).unwrap()
    }

    #[test]
    fn lists_resolve_through_relative_offsets() {
        let b = write(&EditInfo { input: Some(("scenes/x.scene", 7)), special: vec![("Ver", "ManagedResourceCompiler", 1)] });
        assert_eq!((u32_at(&b, INPUT * 8 + 4), u32_at(&b, SPECIAL * 8 + 4)), (1, 1));
        let input = INPUT * 8 + u32_at(&b, INPUT * 8);
        assert_eq!((string_at(&b, input), u32_at(&b, input + 8)), ("scenes/x.scene", 7));
        let special = SPECIAL * 8 + u32_at(&b, SPECIAL * 8);
        assert_eq!(string_at(&b, special + 4), "ManagedResourceCompiler");
    }
}
