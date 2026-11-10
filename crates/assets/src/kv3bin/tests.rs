use super::*;
use kv::{Object, Value};

fn obj(members: Vec<(&str, Value)>) -> Value {
    Value::Object(Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()))
}

fn sample() -> Value {
    let big: Vec<u8> = (0..40_000u32).map(|i| (i % 251) as u8).collect();
    obj(vec![
        ("m_name", Value::String("default_ents".into())),
        ("m_flag", Value::Bool(true)),
        ("zero", Value::Int(0)),
        ("one", Value::Int(1)),
        ("neg", Value::Int(-123_456_789_012)),
        ("half", Value::Double(0.5)),
        ("null", Value::Null),
        ("model", Value::Flagged("resource".into(), Box::new(Value::String("models/dev/box.vmdl".into())))),
        ("vec", Value::Array(vec![Value::Double(1.5), Value::Double(-2.0), Value::Double(0.0)])),
        ("mixed", Value::Array(vec![Value::String("a".into()), Value::Int(7), obj(vec![])])),
        ("empty", Value::Array(vec![])),
        ("blob", Value::Binary(vec![1, 2, 3])),
        ("big", Value::Binary(big)),
        ("nested", obj(vec![("m_name", Value::String("again".into())), ("deep", obj(vec![("x", Value::Double(3.25))]))])),
    ])
}

#[test]
fn round_trips_every_version_and_compression() {
    for version in [4, 5] {
        for c in [Compression::None, Compression::Lz4, Compression::Zstd] {
            let bytes = write(&sample(), version, c);
            assert!(is_binary_kv3(&bytes));
            assert_eq!(read(&bytes), Ok(sample()), "v{version} {c:?}");
        }
    }
}

#[test]
fn round_trips_without_blobs() {
    let v = obj(vec![("a", Value::Array(vec![obj(vec![("b", Value::String("c".into()))])]))]);
    for version in [4, 5] {
        for c in [Compression::None, Compression::Lz4] {
            assert_eq!(read(&write(&v, version, c)), Ok(v.clone()));
        }
    }
}

#[test]
fn rejects_bad_input() {
    assert_eq!(read(b"KV3"), Err(Error::Truncated));
    assert_eq!(read(&0x1234_5678u32.to_le_bytes()), Err(Error::Magic(0x1234_5678)));
    assert_eq!(read(&(MAGIC_BASE | 9).to_le_bytes()), Err(Error::Version(9)));
    let bytes = write(&sample(), 5, Compression::None);
    assert_eq!(read(&bytes[..bytes.len() - 10]), Err(Error::Truncated));
    assert!(!is_binary_kv3(b"<!-- kv3 encoding"));
}

/// v5 typed arrays flagged ARRAY_TYPE_AUXILIARY_BUFFER take their elements from buffer 1's lanes.
#[test]
fn reads_auxiliary_buffer_elements() {
    let mut bytes = write(&obj(vec![("v", Value::Array(vec![Value::Double(2.5)]))]), 5, Compression::None);
    // Rewrite by hand: move the one double into a buffer 1 8-byte lane and mark the array type 25.
    let header_len = 4 + 16 + 4 + 4 + 4 * 4 + 4 + 6 * 4 + 4 * 4 + 8 * 4;
    let count8_1 = 4 + 16 + 4 + 4 + 8;
    bytes[count8_1..count8_1 + 4].copy_from_slice(&1u32.to_le_bytes());
    let buf1_len_at = 4 + 16 + 4 + 4 + 16 + 4 + 24;
    let buf1_len = u32::from_le_bytes(bytes[buf1_len_at..buf1_len_at + 4].try_into().unwrap()) as usize;
    let mut buf1 = bytes[header_len..header_len + buf1_len].to_vec();
    buf1.resize(buf1.len().next_multiple_of(8), 0);
    buf1.extend_from_slice(&2.5f64.to_le_bytes());
    let mut buf2 = bytes[header_len + buf1_len..].to_vec();
    let count8_2 = buf1_len_at + 4 * 4 + 12;
    bytes[count8_2..count8_2 + 4].copy_from_slice(&0u32.to_le_bytes());
    // buffer 2: [object length][b1: count][pad][b4: name id][b8: 2.5][types 9 24 5][trailer]
    let at = buf2.len() - 4 - 3;
    let types = buf2[at..at + 3].to_vec();
    assert_eq!(types, [9, 24, 5]);
    // Drop the 8-byte lane and the padding that aligned it.
    let lane_start = at - 12;
    buf2.drain(lane_start..at);
    buf2[lane_start + 1] = 25;
    bytes[buf1_len_at..buf1_len_at + 4].copy_from_slice(&(buf1.len() as u32).to_le_bytes());
    bytes[buf1_len_at + 8..buf1_len_at + 12].copy_from_slice(&(buf2.len() as u32).to_le_bytes());
    bytes.truncate(header_len);
    bytes.extend_from_slice(&buf1);
    bytes.extend_from_slice(&buf2);
    assert_eq!(read(&bytes), Ok(obj(vec![("v", Value::Array(vec![Value::Double(2.5)]))])));
}
