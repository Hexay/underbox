//! Byte-level helpers shared by the parsers. All offsets are byte offsets into ASCII-structured
//! source; non-ASCII bytes only ever appear inside comments and strings.

pub fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

pub fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The identifier starting at `i`, if any.
pub fn ident_at(s: &str, i: usize) -> Option<&str> {
    let b = s.as_bytes();
    if i >= b.len() || !is_ident_start(b[i]) {
        return None;
    }
    let end = (i..b.len()).find(|&j| !is_ident(b[j])).unwrap_or(b.len());
    Some(&s[i..end])
}

/// Past any whitespace and comments from `i`.
pub fn skip_trivia(s: &str, mut i: usize) -> usize {
    let b = s.as_bytes();
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b[i..].starts_with(b"//") {
            i = s[i..].find('\n').map_or(b.len(), |n| i + n);
        } else if b[i..].starts_with(b"/*") {
            i = s[i + 2..].find("*/").map_or(b.len(), |n| i + n + 4);
        } else {
            return i;
        }
    }
}

/// Past a string or char literal opening at `i` (which holds the quote).
fn skip_literal(b: &[u8], mut i: usize) -> usize {
    let q = b[i];
    i += 1;
    while i < b.len() && b[i] != q && b[i] != b'\n' {
        i += if b[i] == b'\\' { 2 } else { 1 };
    }
    (i + 1).min(b.len())
}

/// `s` with every comment replaced by spaces (newlines kept), so offsets and line numbers hold.
pub fn blank_comments(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => i = skip_literal(b, i),
            b'/' if b.get(i + 1) == Some(&b'/') => {
                let end = s[i..].find('\n').map_or(b.len(), |n| i + n);
                out[i..end].fill(b' ');
                i = end;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let end = s[i + 2..].find("*/").map_or(b.len(), |n| i + n + 4);
                blank_bytes(&mut out[i..end]);
                i = end;
            }
            _ => i += 1,
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Spaces over everything but newlines.
pub fn blank_bytes(b: &mut [u8]) {
    for c in b.iter_mut().filter(|c| **c != b'\n') {
        *c = b' ';
    }
}

/// Index of the bracket closing the one at `open`, skipping literals. Expects comment-free input
/// unless `comments` is set.
pub fn match_close(s: &str, open: usize, comments: bool) -> Option<usize> {
    let b = s.as_bytes();
    let (o, c) = match b[open] {
        b'{' => (b'{', b'}'),
        b'(' => (b'(', b')'),
        b'[' => (b'[', b']'),
        _ => return None,
    };
    let mut depth = 0usize;
    let mut i = open;
    while i < b.len() {
        if comments && (b[i..].starts_with(b"//") || b[i..].starts_with(b"/*")) {
            i = skip_trivia(s, i);
            continue;
        }
        match b[i] {
            b'"' | b'\'' => {
                i = skip_literal(b, i);
                continue;
            }
            x if x == o => depth += 1,
            x if x == c => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Split on top-level commas (outside brackets and literals); pieces are trimmed.
pub fn split_args(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let (mut depth, mut start, mut out) = (0i32, 0, Vec::new());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => {
                i = skip_literal(b, i);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                out.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if !s[start..].trim().is_empty() || !out.is_empty() {
        out.push(s[start..].trim());
    }
    out
}

/// Every ident occurrence of `word` (whole word) in `s`.
pub fn occurrences<'a>(s: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    s.match_indices(word).map(|(i, _)| i).filter(move |&i| (i == 0 || !is_ident(s.as_bytes()[i - 1])) && ident_at(s, i) == Some(word))
}

/// `<keyword> < T > B` at `i` (where `keyword` occurs): T, B and the span of T.
pub fn buffer_declaration<'a>(s: &'a str, i: usize, keyword: &str) -> Option<(&'a str, &'a str, (usize, usize))> {
    let lt = skip_trivia(s, i + keyword.len());
    (s.as_bytes().get(lt) == Some(&b'<')).then_some(())?;
    let at = skip_trivia(s, lt + 1);
    let ty = ident_at(s, at)?;
    let gt = skip_trivia(s, at + ty.len());
    (s.as_bytes().get(gt) == Some(&b'>')).then_some(())?;
    let name = ident_at(s, skip_trivia(s, gt + 1))?;
    Some((ty, name, (at, at + ty.len())))
}

/// The `;` ending the statement that continues at `i`; `None` if a bracket closes first.
pub fn statement_end(s: &str, mut i: usize) -> Option<usize> {
    let b = s.as_bytes();
    while i < b.len() {
        match b[i] {
            b';' => return Some(i),
            b'(' | b'[' | b'{' => i = match_close(s, i, false)?,
            b')' | b']' | b'}' => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

/// `=` at `i` (not `==`).
pub fn is_assign(s: &str, i: usize) -> bool {
    let b = s.as_bytes();
    b.get(i) == Some(&b'=') && b.get(i + 1) != Some(&b'=')
}

/// A compound assignment or `++`/`--` at `i`.
pub fn is_compound_assign(s: &str, i: usize) -> bool {
    let b = s.as_bytes();
    let op = |n: usize| b.get(i + n).copied();
    matches!((op(0), op(1), op(2)), (Some(b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^'), Some(b'='), _) | (Some(b'<'), Some(b'<'), Some(b'=')) | (Some(b'>'), Some(b'>'), Some(b'=')))
        || matches!((op(0), op(1)), (Some(b'+'), Some(b'+')) | (Some(b'-'), Some(b'-')))
}

/// 1-based line of byte offset `i`.
pub fn line_of(s: &str, i: usize) -> u32 {
    1 + s.as_bytes()[..i].iter().filter(|&&c| c == b'\n').count() as u32
}

/// `"text"` -> `text`; anything else unchanged.
pub fn unquote(s: &str) -> &str {
    let t = s.trim();
    t.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(t)
}
