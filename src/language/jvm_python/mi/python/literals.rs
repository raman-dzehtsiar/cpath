// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT
fn decimal_hex(digits: &str) -> Option<String> {
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // The pinned CPython default rejects longer decimal integer literals.
    // Keep unsupported inputs linear instead of doing unbounded conversion.
    if digits.len() > 4300 {
        return Some(format!("decimal:{}", digits.trim_start_matches('0')));
    }
    let mut limbs = vec![0_u32];
    for chunk in digits.as_bytes().chunks(9) {
        let multiplier = 10_u64.pow(chunk.len() as u32);
        let mut carry = chunk
            .iter()
            .fold(0_u64, |value, digit| value * 10 + u64::from(digit - b'0'));
        for limb in &mut limbs {
            let value = u64::from(*limb) * multiplier + carry;
            *limb = value as u32;
            carry = value >> 32;
        }
        if carry != 0 {
            limbs.push(carry as u32);
        }
    }
    let mut result = format!("{:x}", limbs.pop()?);
    for limb in limbs.into_iter().rev() {
        result.push_str(&format!("{limb:08x}"));
    }
    Some(result)
}

fn power_radix_hex(digits: &str, radix: u32) -> Option<String> {
    let bits = radix.ilog2();
    let mut pending = 0_u32;
    let mut width = 0;
    let mut reversed = Vec::new();
    for digit in digits.chars().rev() {
        pending |= digit.to_digit(radix)? << width;
        width += bits;
        while width >= 4 {
            reversed.push(b"0123456789abcdef"[(pending & 15) as usize]);
            pending >>= 4;
            width -= 4;
        }
    }
    if pending != 0 {
        reversed.push(b"0123456789abcdef"[pending as usize]);
    }
    while reversed.last() == Some(&b'0') {
        reversed.pop();
    }
    if reversed.is_empty() {
        return Some("0".into());
    }
    reversed.reverse();
    String::from_utf8(reversed).ok()
}

fn integer_hex(digits: &str, radix: u32) -> Option<String> {
    if let Ok(integer) = u128::from_str_radix(digits, radix) {
        return Some(format!("{integer:x}"));
    }
    if radix == 10 {
        decimal_hex(digits)
    } else {
        power_radix_hex(digits, radix)
    }
}

fn float_identity(number: f64) -> String {
    if number == 0.0 {
        return "0".into();
    }
    if !number.is_finite() || number.fract() != 0.0 {
        return number.to_string();
    }
    let bits = number.abs().to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32 - 1023;
    let significand = (bits & ((1_u64 << 52) - 1)) | (1_u64 << 52);
    let mut result = if exponent < 52 {
        format!("{:x}", significand >> (52 - exponent))
    } else {
        let shift = (exponent - 52) as usize;
        let mut value = format!("{:x}", significand << (shift % 4));
        value.push_str(&"0".repeat(shift / 4));
        value
    };
    if number.is_sign_negative() {
        result.insert(0, '-');
    }
    result
}

pub(super) fn numeric(value: &str) -> Option<String> {
    let normalized = value.replace('_', "").to_ascii_lowercase();
    if let Some(imaginary) = normalized.strip_suffix('j') {
        let number = imaginary.parse::<f64>().ok()?;
        return Some(if number == 0.0 {
            "0".into()
        } else {
            format!("imaginary:{}", float_identity(number))
        });
    }
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(digits) = normalized.strip_prefix(prefix) {
            return integer_hex(digits, radix);
        }
    }
    if normalized.bytes().all(|byte| byte.is_ascii_digit()) {
        return integer_hex(&normalized, 10);
    }
    normalized.parse::<f64>().ok().map(float_identity)
}

fn push_codepoint(output: &mut String, codepoint: u32) -> Option<()> {
    match codepoint {
        0 => output.push_str("\0\0"),
        0xd800..=0xdfff => output.push_str(&format!("\0{codepoint:x};")),
        _ => output.push(char::from_u32(codepoint)?),
    }
    Some(())
}

pub(super) fn string_value(source: &str) -> Option<String> {
    let quote = source.find(['\'', '"'])?;
    let prefix = source[..quote].to_ascii_lowercase();
    if prefix.contains('f') || prefix.contains('t') {
        return None;
    }
    let bytes = prefix.contains('b');
    let delimiter = source.as_bytes()[quote];
    let triple = source
        .as_bytes()
        .get(quote..quote + 3)
        .is_some_and(|part| part.iter().all(|byte| *byte == delimiter));
    let size = if triple { 3 } else { 1 };
    let body = &source[quote + size..source.len().checked_sub(size)?];
    let mut output = String::new();
    let mut characters = body.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\\' || prefix.contains('r') {
            push_codepoint(&mut output, character as u32)?;
            continue;
        }
        let escaped = characters.next()?;
        match escaped {
            '\n' => {}
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
            }
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            'a' => output.push('\u{7}'),
            'b' => output.push('\u{8}'),
            'f' => output.push('\u{c}'),
            'v' => output.push('\u{b}'),
            '\\' | '\'' | '"' => output.push(escaped),
            'x' | 'u' | 'U' if !bytes || escaped == 'x' => {
                let count = match escaped {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                };
                let digits: String = characters.by_ref().take(count).collect();
                push_codepoint(&mut output, u32::from_str_radix(&digits, 16).ok()?)?;
            }
            '0'..='7' => {
                let mut digits = escaped.to_string();
                for _ in 0..2 {
                    if characters
                        .peek()
                        .is_some_and(|next| matches!(next, '0'..='7'))
                    {
                        digits.push(characters.next()?);
                    }
                }
                let mut value = u32::from_str_radix(&digits, 8).ok()?;
                if bytes {
                    value %= 256;
                }
                push_codepoint(&mut output, value)?;
            }
            _ => {
                output.push('\\');
                output.push(escaped);
            }
        }
    }
    Some(format!("{}:{output}", if bytes { "bytes" } else { "str" }))
}
