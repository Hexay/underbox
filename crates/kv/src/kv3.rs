//! KeyValues3 text: an optional `<!-- kv3 encoding:... format:... -->` header, then one value.
//! Objects are `{ key = value }`, arrays `[ a, b ]`, strings `"x"` or multi-line `"""..."""`,
//! binary `#[ 0a ff ]`, and flagged values `resource:"x"` / `resource_name:"x"`.

use crate::cursor::Cursor;
use crate::{Error, Object, Value};

/// The header's `encoding:` and `format:` names with their GUIDs, e.g. ("modeldoc29", "3cec427c-...").
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    pub encoding: Option<(String, String)>,
    pub format: Option<(String, String)>,
}

pub fn parse(src: &str) -> Result<(Header, Value), Error> {
    let mut c = Cursor::new(src.trim_start_matches('\u{feff}'));
    c.skip_trivia();
    let header = if c.starts_with("<!--") { header(&mut c)? } else { Header::default() };
    let v = value(&mut c)?;
    c.skip_trivia();
    if c.peek().is_some() {
        return c.err("trailing content after the root value");
    }
    Ok((header, v))
}

fn header(c: &mut Cursor) -> Result<Header, Error> {
    c.expect("<!--")?;
    let Some(text) = c.take_until_str("-->") else { return c.err("unterminated header") };
    c.expect("-->")?;
    // `name:value:version{guid}` → (value, guid)
    let field = |name: &str| {
        let rest = text.split_whitespace().find_map(|w| w.strip_prefix(name)?.strip_prefix(':'))?;
        let (kind, guid) = rest.split_once(":version{")?;
        Some((kind.to_owned(), guid.trim_end_matches('}').to_owned()))
    };
    Ok(Header { encoding: field("encoding"), format: field("format") })
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'+')
}

fn value(c: &mut Cursor) -> Result<Value, Error> {
    c.skip_trivia();
    match c.peek() {
        Some(b'{') => object(c).map(Value::Object),
        Some(b'[') => array(c),
        Some(b'#') => binary(c),
        Some(b'"') if c.starts_with("\"\"\"") => multiline(c).map(Value::String),
        Some(b'"') => c.quoted().map(Value::String),
        Some(b) if is_ident(b) => {
            let word = c.take_until(|b| !is_ident(b)).to_owned();
            if c.peek() == Some(b':') {
                c.bump();
                return Ok(Value::Flagged(word, Box::new(value(c)?)));
            }
            literal(c, &word)
        }
        _ => c.err("expected a value"),
    }
}

fn literal(c: &Cursor, word: &str) -> Result<Value, Error> {
    Ok(match word {
        "null" => Value::Null,
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ if let Ok(i) = word.parse::<i64>() => Value::Int(i),
        _ if let Ok(d) = word.parse::<f64>() => Value::Double(d),
        _ => return c.err(format!("unknown literal `{word}`")),
    })
}

fn object(c: &mut Cursor) -> Result<Object, Error> {
    c.expect("{")?;
    let mut obj = Object::default();
    loop {
        c.skip_trivia();
        match c.peek() {
            Some(b'}') => {
                c.bump();
                return Ok(obj);
            }
            Some(b'"') => {
                let key = c.quoted()?;
                obj.0.push(member(c, key)?);
            }
            Some(b) if is_ident(b) => {
                let key = c.take_until(|b| !is_ident(b)).to_owned();
                obj.0.push(member(c, key)?);
            }
            _ => return c.err("expected a key or `}`"),
        }
    }
}

fn member(c: &mut Cursor, key: String) -> Result<(String, Value), Error> {
    c.skip_trivia();
    c.expect("=")?;
    Ok((key, value(c)?))
}

fn array(c: &mut Cursor) -> Result<Value, Error> {
    c.expect("[")?;
    let mut items = Vec::new();
    loop {
        c.skip_trivia();
        match c.peek() {
            Some(b']') => {
                c.bump();
                return Ok(Value::Array(items));
            }
            Some(b',') => {
                c.bump();
            }
            None => return c.err("unterminated array"),
            _ => items.push(value(c)?),
        }
    }
}

fn binary(c: &mut Cursor) -> Result<Value, Error> {
    c.expect("#[")?;
    let hex: String = c.take_until(|b| b == b']').split_whitespace().collect();
    c.expect("]")?;
    if !hex.len().is_multiple_of(2) {
        return c.err("odd number of hex digits in binary blob");
    }
    let bytes = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16));
    match bytes.collect::<Result<Vec<_>, _>>() {
        Ok(b) => Ok(Value::Binary(b)),
        Err(_) => c.err("bad hex digit in binary blob"),
    }
}

/// `"""` newline, text, newline `"""`: the newlines next to the quotes aren't part of the string.
fn multiline(c: &mut Cursor) -> Result<String, Error> {
    c.expect("\"\"\"")?;
    let Some(text) = c.take_until_str("\"\"\"") else { return c.err("unterminated multi-line string") };
    c.expect("\"\"\"")?;
    let text = text.strip_prefix("\r\n").or_else(|| text.strip_prefix('\n')).unwrap_or(&text);
    let text = text.strip_suffix("\r\n").or_else(|| text.strip_suffix('\n')).unwrap_or(text);
    Ok(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features() {
        let src = "<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->\n\
            { a = 1 b = -2.5 c = [ true, null, ] \"d e\" = resource:\"x.vmat\"\n\
            f = \"\"\"\nline ü\n\"\"\" g = #[ 0a FF ] // note\n /* block */ }";
        let (h, v) = parse(src).unwrap();
        assert_eq!(h.format, Some(("generic".into(), "7412167c-06e9-4698-aff2-e63eb59037e7".into())));
        let o = v.as_object().unwrap();
        assert_eq!(o.get("a"), Some(&Value::Int(1)));
        assert_eq!(o.get("b"), Some(&Value::Double(-2.5)));
        assert_eq!(o.get("c"), Some(&Value::Array(vec![Value::Bool(true), Value::Null])));
        assert_eq!(o.get("d e"), Some(&Value::Flagged("resource".into(), Box::new(Value::String("x.vmat".into())))));
        assert_eq!(o.str("f"), Some("line ü"));
        assert_eq!(o.get("g"), Some(&Value::Binary(vec![0x0a, 0xff])));
    }
}
