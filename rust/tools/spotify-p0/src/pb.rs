//! Minimal protobuf wire helpers for messages LibreSpot 0.8.0 does not compile
//! (collection2v2.proto). Only what the spike needs.

pub fn put_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            buf.push(byte);
            return;
        }
        buf.push(byte | 0x80);
    }
}

pub fn put_str(buf: &mut Vec<u8>, field: u32, value: &str) {
    put_varint(buf, u64::from(field << 3 | 2));
    put_varint(buf, value.len() as u64);
    buf.extend_from_slice(value.as_bytes());
}

pub fn put_bytes(buf: &mut Vec<u8>, field: u32, value: &[u8]) {
    put_varint(buf, u64::from(field << 3 | 2));
    put_varint(buf, value.len() as u64);
    buf.extend_from_slice(value);
}

pub fn put_int(buf: &mut Vec<u8>, field: u32, value: u64) {
    put_varint(buf, u64::from(field << 3));
    put_varint(buf, value);
}

pub enum Value<'a> {
    #[cfg_attr(not(test), allow(dead_code))]
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

fn get_varint(data: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let byte = *data.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// Top-level fields of a message as (field number, value).
pub fn fields(data: &[u8]) -> Option<Vec<(u32, Value<'_>)>> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let key = get_varint(data, &mut pos)?;
        let field = (key >> 3) as u32;
        match key & 7 {
            0 => out.push((field, Value::Varint(get_varint(data, &mut pos)?))),
            1 => {
                pos += 8;
                out.push((field, Value::Fixed));
            }
            2 => {
                let len = get_varint(data, &mut pos)? as usize;
                let bytes = data.get(pos..pos + len)?;
                pos += len;
                out.push((field, Value::Bytes(bytes)));
            }
            5 => {
                pos += 4;
                out.push((field, Value::Fixed));
            }
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
pub fn string_field(data: &[u8], wanted: u32) -> Option<String> {
    fields(data)?
        .into_iter()
        .find_map(|(field, value)| match value {
            Value::Bytes(bytes) if field == wanted => String::from_utf8(bytes.to_vec()).ok(),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_string_and_int() {
        let mut buf = Vec::new();
        put_str(&mut buf, 2, "collection");
        put_int(&mut buf, 4, 300);
        let parsed = fields(&buf).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(string_field(&buf, 2).as_deref(), Some("collection"));
        assert!(matches!(parsed[1], (4, Value::Varint(300))));
    }
}
