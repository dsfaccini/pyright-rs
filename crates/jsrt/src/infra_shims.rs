//! Stand-ins for JS string and number semantics that the infra branch needs
//! before the jsrt branch lands its versions. Each one is replaced by the jsrt
//! helper of the same behavior, then this file is deleted.

use std::cmp::Ordering;

/// JS `a < b` order on strings: UTF-16 code unit order, not UTF-8 byte order.
pub fn utf16_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// JS `s.length`.
pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// Byte offset of UTF-16 offset `index`, clamped to the string end. An index
/// inside a surrogate pair rounds up to the end of the pair.
pub fn utf16_to_byte(s: &str, index: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in s.char_indices() {
        if units >= index {
            return byte;
        }
        units += ch.len_utf16();
    }
    s.len()
}

/// JS `s.substring(start, end)` with UTF-16 offsets.
pub fn substring(s: &str, start: usize, end: Option<usize>) -> String {
    let len = utf16_len(s);
    let mut start = start.min(len);
    let mut end = end.unwrap_or(len).min(len);
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }
    s[utf16_to_byte(s, start)..utf16_to_byte(s, end)].to_string()
}

/// JS `s.slice(start, end)` with UTF-16 offsets; negative offsets count from the end.
pub fn slice(s: &str, start: isize, end: Option<isize>) -> String {
    let len = utf16_len(s) as isize;
    let resolve = |i: isize| if i < 0 { (len + i).max(0) } else { i.min(len) };
    let start = resolve(start);
    let end = resolve(end.unwrap_or(len));
    if start >= end {
        return String::new();
    }
    s[utf16_to_byte(s, start as usize)..utf16_to_byte(s, end as usize)].to_string()
}

/// JS `s.toLowerCase()`.
pub fn to_lower_case(s: &str) -> String {
    s.to_lowercase()
}

/// JS `s.toUpperCase()`.
pub fn to_upper_case(s: &str) -> String {
    s.to_uppercase()
}

/// JS `\s` and `String.prototype.trim` whitespace.
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{0B}'
            | '\u{0C}'
            | '\r'
            | ' '
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// JS `s.trim()`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// JS `haystack.replace(/needle/g, replacement)` for a needle with no regex
/// metacharacters and no capture groups. Expands `$$`, `$&`, `` $` `` and `$'`
/// in `replacement` the way JS does; any other `$` stays literal.
pub fn replace_all_literal(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || !haystack.contains(needle) {
        return haystack.to_string();
    }
    let mut out = String::with_capacity(haystack.len());
    let mut last = 0;
    for (pos, matched) in haystack.match_indices(needle) {
        out.push_str(&haystack[last..pos]);
        let after = pos + matched.len();
        let mut chars = replacement.char_indices().peekable();
        while let Some((_, ch)) = chars.next() {
            if ch == '$' {
                match chars.peek().map(|&(_, c)| c) {
                    Some('$') => {
                        out.push('$');
                        chars.next();
                    }
                    Some('&') => {
                        out.push_str(matched);
                        chars.next();
                    }
                    Some('`') => {
                        out.push_str(&haystack[..pos]);
                        chars.next();
                    }
                    Some('\'') => {
                        out.push_str(&haystack[after..]);
                        chars.next();
                    }
                    _ => out.push('$'),
                }
            } else {
                out.push(ch);
            }
        }
        last = after;
    }
    out.push_str(&haystack[last..]);
    out
}

/// JS `String(n)` for a finite or non-finite number.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n == 0.0 {
        return "0".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if n < 0.0 {
        return format!("-{}", number_to_string(-n));
    }
    // Shortest round-trip digits, as `d.ddde<exp>`.
    let sci = format!("{n:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let point = exp + 1; // ECMA-262 `n`
    if k <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let sign = if point - 1 < 0 { '-' } else { '+' };
        let e = (point - 1).abs();
        if k == 1 { format!("{digits}e{sign}{e}") } else { format!("{}.{}e{sign}{e}", &digits[..1], &digits[1..]) }
    }
}

/// JS `JSON.stringify(s)` for a string.
pub fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_like_js() {
        for (n, s) in [
            (118.352, "118.352"),
            (1.0, "1"),
            (0.1, "0.1"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (1.5e-7, "1.5e-7"),
            (-2.5, "-2.5"),
        ] {
            assert_eq!(number_to_string(n), s);
        }
    }

    #[test]
    fn replace_expands_js_patterns() {
        assert_eq!(replace_all_literal("a {x} b {x}", "{x}", "$&-$$"), "a {x}-$ b {x}-$");
        assert_eq!(replace_all_literal("<{x}>", "{x}", "$`|$'"), "<<|>>");
        assert_eq!(replace_all_literal("{x}", "{x}", "$1"), "$1");
    }

    #[test]
    fn substring_uses_utf16_offsets() {
        assert_eq!(substring("a\u{1F600}b", 1, Some(3)), "\u{1F600}");
        assert_eq!(slice("abcdef", -2, None), "ef");
    }
}
