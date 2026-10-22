// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Strict source decoding; no guessed code pages or replacement characters.
use std::borrow::Cow;

pub fn decode(bytes: &[u8]) -> Result<Cow<'_, str>, String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little_endian = bytes[0] == 0xff;
        let payload = &bytes[2..];
        if !payload.len().is_multiple_of(2) {
            return Err("truncated BOM-marked UTF-16 source".into());
        }
        let units = payload.as_chunks::<2>().0.iter().map(|b| {
            if little_endian {
                u16::from_le_bytes([b[0], b[1]])
            } else {
                u16::from_be_bytes([b[0], b[1]])
            }
        });
        let source: Result<String, _> = char::decode_utf16(units).collect();
        return source
            .map(Cow::Owned)
            .map_err(|_| "invalid UTF-16 surrogate sequence".into());
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    std::str::from_utf8(bytes)
        .map(Cow::Borrowed)
        .map_err(|_| "unsupported encoding: expected UTF-8 or BOM-marked UTF-16".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_encodings_preserve_unicode_and_lines() {
        let source = "class Café { string s = \"🌍\"; }\r\n";
        assert_eq!(decode(source.as_bytes()).unwrap(), source);
        let mut utf8 = vec![0xef, 0xbb, 0xbf];
        utf8.extend_from_slice(source.as_bytes());
        assert_eq!(decode(&utf8).unwrap(), source);
        for little in [false, true] {
            let mut bytes = if little {
                vec![0xff, 0xfe]
            } else {
                vec![0xfe, 0xff]
            };
            for unit in source.encode_utf16() {
                bytes.extend(if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            assert_eq!(decode(&bytes).unwrap(), source);
        }
    }

    #[test]
    fn malformed_encodings_are_never_lossily_replaced() {
        for bytes in [
            &[0xff][..],
            &[0xff, 0xfe, 0],
            &[0xff, 0xfe, 0, 0xd8],
            &[0xfe, 0xff, 0xdc, 0],
        ] {
            assert!(decode(bytes).is_err());
        }
    }
}
