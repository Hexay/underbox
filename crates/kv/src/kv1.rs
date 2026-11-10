//! KeyValues1 text: `"key" "value"` pairs and `"key" { ... }` blocks. Tokens may be unquoted,
//! and a `[$PLATFORM]` conditional may follow a value (ignored: every platform's value is kept).

use crate::cursor::Cursor;
use crate::{Error, Object, Value};

/// Parse a file: one or more top-level `name { ... }` blocks, as an object of them.
pub fn parse(src: &str) -> Result<Object, Error> {
    let mut c = Cursor::new(src.trim_start_matches('\u{feff}'));
    let mut root = Object::default();
    loop {
        c.skip_trivia();
        if c.peek().is_none() {
            return Ok(root);
        }
        root.0.push(pair(&mut c)?);
    }
}

fn token(c: &mut Cursor) -> Result<String, Error> {
    c.skip_trivia();
    match c.peek() {
        Some(b'"') => c.quoted(),
        Some(b'{' | b'}') | None => c.err("expected a key or value"),
        Some(_) => Ok(c.take_until(|b| b.is_ascii_whitespace() || matches!(b, b'"' | b'{' | b'}')).to_owned()),
    }
}

fn pair(c: &mut Cursor) -> Result<(String, Value), Error> {
    let key = token(c)?;
    c.skip_trivia();
    let value = if c.peek() == Some(b'{') { Value::Object(block(c)?) } else { Value::String(token(c)?) };
    skip_conditional(c);
    Ok((key, value))
}

fn block(c: &mut Cursor) -> Result<Object, Error> {
    c.expect("{")?;
    let mut obj = Object::default();
    loop {
        c.skip_trivia();
        match c.peek() {
            Some(b'}') => {
                c.bump();
                return Ok(obj);
            }
            None => return c.err("unterminated block"),
            _ => obj.0.push(pair(c)?),
        }
    }
}

fn skip_conditional(c: &mut Cursor) {
    // Only same-line whitespace may separate a value from its conditional.
    while matches!(c.peek(), Some(b' ' | b'\t')) {
        c.bump();
    }
    if c.peek() == Some(b'[') {
        c.take_until(|b| b == b']' || b == b'\n');
        if c.peek() == Some(b']') {
            c.bump();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vmat() {
        let src = "Layer0\n{\n\tshader \"x.shader_c\"\n\t\"g_v\" \"[1 2]\" [$WIN]\n\tlegacy { \"$a\" \"1\" }\n}\n";
        let root = parse(src).unwrap();
        let layer = root.get("layer0").unwrap().as_object().unwrap();
        assert_eq!(layer.str("shader"), Some("x.shader_c"));
        assert_eq!(layer.str("g_v"), Some("[1 2]"));
        assert_eq!(layer.get("legacy").unwrap().as_object().unwrap().str("$a"), Some("1"));
    }
}
