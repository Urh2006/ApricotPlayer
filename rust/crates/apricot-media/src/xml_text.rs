//! Strict XML byte decoding shared by remote feeds and local OPML imports.

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum XmlTextError {
    #[error("XML uses an unsupported or malformed text encoding")]
    InvalidEncoding,
}

/// Decodes one XML document without lossy replacement characters.
///
/// # Errors
///
/// Returns an error when the declared encoding is unknown or the bytes are not
/// valid in the detected encoding.
pub fn decode_xml(bytes: &[u8]) -> Result<String, XmlTextError> {
    let (encoding, content) = if let Some(content) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (UTF_8, content)
    } else if let Some(content) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        (UTF_16LE, content)
    } else if let Some(content) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (UTF_16BE, content)
    } else if bytes.starts_with(&[0x3C, 0x00, 0x3F, 0x00]) {
        (UTF_16LE, bytes)
    } else if bytes.starts_with(&[0x00, 0x3C, 0x00, 0x3F]) {
        (UTF_16BE, bytes)
    } else {
        let encoding = match declared_encoding(bytes) {
            Some(label) => {
                Encoding::for_label(label.as_bytes()).ok_or(XmlTextError::InvalidEncoding)?
            }
            None => UTF_8,
        };
        (encoding, bytes)
    };
    encoding
        .decode_without_bom_handling_and_without_replacement(content)
        .map(|xml| xml.trim_start_matches('\u{feff}').to_owned())
        .ok_or(XmlTextError::InvalidEncoding)
}

fn declared_encoding(bytes: &[u8]) -> Option<String> {
    let prefix = &bytes[..bytes.len().min(512)];
    let ascii = prefix
        .iter()
        .map(|byte| {
            if byte.is_ascii() {
                char::from(*byte).to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>();
    let declaration_end = ascii.find("?>")?;
    let declaration = &ascii[..declaration_end];
    let encoding_at = declaration.find("encoding")? + "encoding".len();
    let value = declaration[encoding_at..].trim_start();
    let value = value.strip_prefix('=')?.trim_start();
    let quote = value.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let value = &value[quote.len_utf8()..];
    let end = value.find(quote)?;
    let label = value[..end].trim();
    (!label.is_empty()).then(|| label.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{XmlTextError, declared_encoding, decode_xml};

    #[test]
    fn decodes_utf_boms_and_declared_legacy_encodings_without_replacement() {
        assert_eq!(
            decode_xml(&[0xEF, 0xBB, 0xBF, b'<', b'r', b's', b's', b'/', b'>']).expect("UTF-8 BOM"),
            "<rss/>"
        );
        let utf16 = "<?xml version=\"1.0\"?><rss/>"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            decode_xml(&utf16).expect("UTF-16"),
            "<?xml version=\"1.0\"?><rss/>"
        );
        assert!(
            decode_xml(b"<?xml version='1.0' encoding='windows-1252'?><rss>\x80</rss>")
                .expect("Windows-1252")
                .contains('\u{20ac}')
        );
        assert_eq!(
            decode_xml(b"<?xml version='1.0'?><rss>\xff</rss>").expect_err("bad UTF-8"),
            XmlTextError::InvalidEncoding
        );
    }

    #[test]
    fn encoding_parser_is_scoped_to_the_xml_declaration() {
        assert_eq!(
            declared_encoding(b"<?xml version='1.0' encoding = \"ISO-8859-1\"?><rss/>").as_deref(),
            Some("iso-8859-1")
        );
        assert_eq!(
            declared_encoding(b"<rss><encoding>utf-16</encoding></rss>"),
            None
        );
    }
}
