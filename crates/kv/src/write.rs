//! KeyValues3 text output, readable by [`crate::kv3::parse`] and by Source 2's own KV3 loader.

use crate::{Object, Value};
use std::fmt::Write;

pub const TEXT_HEADER: &str = "<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} \
    format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->";

/// A whole KV3 text document: the generic-format header, then `value`.
pub fn kv3(value: &Value) -> String {
    let mut out = String::from(TEXT_HEADER);
    out.push('\n');
    write_value(&mut out, value, 0);
    out.push('\n');
    out
}

fn indent(out: &mut String, depth: usize) {
    (0..depth).for_each(|_| out.push('\t'));
}

fn write_value(out: &mut String, v: &Value, depth: usize) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => write!(out, "{i}").unwrap(),
        // `{:?}` always keeps a `.` or exponent, so it reads back as a double rather than an int.
        Value::Double(d) => write!(out, "{d:?}").unwrap(),
        Value::String(s) => string(out, s),
        Value::Binary(b) => {
            out.push_str("#[");
            b.iter().for_each(|b| write!(out, " {b:02x}").unwrap());
            out.push_str(" ]");
        }
        Value::Flagged(flag, v) => {
            write!(out, "{flag}:").unwrap();
            write_value(out, v, depth);
        }
        Value::Array(items) => array(out, items, depth),
        Value::Object(o) => object(out, o, depth),
    }
}

fn array(out: &mut String, items: &[Value], depth: usize) {
    if items.is_empty() {
        out.push_str("[ ]");
        return;
    }
    out.push_str("[\n");
    for item in items {
        indent(out, depth + 1);
        write_value(out, item, depth + 1);
        out.push_str(",\n");
    }
    indent(out, depth);
    out.push(']');
}

fn object(out: &mut String, o: &Object, depth: usize) {
    if o.0.is_empty() {
        out.push_str("{ }");
        return;
    }
    out.push_str("{\n");
    for (key, v) in &o.0 {
        indent(out, depth + 1);
        let bare = !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') && !key.as_bytes()[0].is_ascii_digit();
        if bare { out.push_str(key) } else { string(out, key) }
        out.push_str(" = ");
        write_value(out, v, depth + 1);
        out.push('\n');
    }
    indent(out, depth);
    out.push('}');
}

fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv3::parse;

    #[test]
    fn round_trips_through_the_parser() {
        let value = Value::Object(Object(vec![
            ("name".into(), Value::String("mesh#0 \"q\" \\ \n".into())),
            ("two words".into(), Value::Int(-3)),
            ("ratio".into(), Value::Double(1.0)),
            ("big".into(), Value::Double(1e20)),
            ("list".into(), Value::Array(vec![Value::Bool(true), Value::Null, Value::Array(vec![])])),
            ("blob".into(), Value::Binary(vec![0, 0xab])),
            ("model".into(), Value::Flagged("resource".into(), Box::new(Value::String("a.vmdl".into())))),
            ("empty".into(), Value::Object(Object::default())),
        ]));
        let text = kv3(&value);
        assert!(text.starts_with("<!-- kv3 encoding:text"));
        let (header, parsed) = parse(&text).unwrap();
        assert_eq!(header.format.unwrap().0, "generic");
        assert_eq!(parsed, value);
    }
}
