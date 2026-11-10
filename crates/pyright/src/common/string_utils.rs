//! Port of `packages/pyright-internal/src/common/stringUtils.ts`.
//!
//! `isPatternInSymbol` serves completions only and is not ported.

use crate::common::core::{Comparison, compare_comparable_values_str};
use jsrt::JsStr;

// common/stringUtils.ts:104
/// Hashes the UTF-16 code units of `contents`, as JS `charCodeAt` sees them.
pub fn hash_string(contents: &str) -> i32 {
    hash_string_utf16(contents.encode_utf16())
}

// common/stringUtils.ts:104
/// `hashString` over UTF-16 code units, for text that may hold lone surrogates.
pub fn hash_string_utf16(contents: impl IntoIterator<Item = u16>) -> i32 {
    let mut hash: i32 = 0;

    for code_unit in contents {
        // `(hash << 5) - hash + c | 0`: every step wraps to 32 bits.
        hash = (hash << 5)
            .wrapping_sub(hash)
            .wrapping_add(code_unit as i32);
    }
    hash
}

// common/stringUtils.ts:113
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringFingerprint {
    pub primary: i32,
    pub secondary: i32,
}

// common/stringUtils.ts:119
pub fn get_string_fingerprint(contents: &str) -> StringFingerprint {
    let mut primary: i32 = 0;
    let mut secondary: i32 = 0x811c9dc5_u32 as i32;

    for code_unit in contents.encode_utf16() {
        primary = (primary << 5)
            .wrapping_sub(primary)
            .wrapping_add(code_unit as i32);
        secondary = (secondary ^ code_unit as i32).wrapping_mul(0x01000193);
    }

    StringFingerprint { primary, secondary }
}

// common/stringUtils.ts:132
pub fn are_string_fingerprints_equal(left: &StringFingerprint, right: &StringFingerprint) -> bool {
    left.primary == right.primary && left.secondary == right.secondary
}

// common/stringUtils.ts:148
pub fn compare_strings_case_insensitive(a: Option<&str>, b: Option<&str>) -> Comparison {
    match (a, b) {
        _ if a == b => Comparison::EqualTo,
        (None, _) => Comparison::LessThan,
        (_, None) => Comparison::GreaterThan,
        (Some(a), Some(b)) => {
            let a = a.js_to_upper_case();
            let b = b.js_to_upper_case();
            compare_comparable_values_str(Some(&a), Some(&b))
        }
    }
}

// common/stringUtils.ts:168
pub fn compare_strings_case_sensitive(a: Option<&str>, b: Option<&str>) -> Comparison {
    compare_comparable_values_str(a, b)
}

// common/stringUtils.ts:172
pub fn get_string_comparer(ignore_case: bool) -> fn(Option<&str>, Option<&str>) -> Comparison {
    if ignore_case {
        compare_strings_case_insensitive
    } else {
        compare_strings_case_sensitive
    }
}

// common/stringUtils.ts:184
pub fn equate_strings_case_insensitive(a: &str, b: &str) -> bool {
    compare_strings_case_insensitive(Some(a), Some(b)) == Comparison::EqualTo
}

// common/stringUtils.ts:194
pub fn equate_strings_case_sensitive(a: &str, b: &str) -> bool {
    compare_strings_case_sensitive(Some(a), Some(b)) == Comparison::EqualTo
}

// common/stringUtils.ts:198
/// Counts the UTF-16 code units of `value` equal to `ch`, a single code unit.
pub fn get_character_count(value: &str, ch: char) -> i32 {
    let mut result = 0;
    for c in value.chars() {
        if c == ch {
            result += 1;
        }
    }
    result
}

// common/stringUtils.ts:208
pub fn get_last_dotted_string(text: &str) -> &str {
    match text.rfind('.') {
        Some(index) if index > 0 => &text[index + 1..],
        _ => text,
    }
}

// common/stringUtils.ts:213
pub fn truncate(text: &str, max_length: usize) -> String {
    if text.js_len() as usize > max_length {
        return format!(
            "{}...",
            text.js_substring(0, (max_length - "...".len()) as i32)
        );
    }
    text.to_string()
}

// common/stringUtils.ts:220
pub fn escape_reg_exp(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '^' | '$' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported from tests/stringUtils.test.ts.
    #[test]
    fn core_compare_strings_case_insensitive1() {
        assert_eq!(
            compare_strings_case_insensitive(Some("Hello"), Some("hello")),
            Comparison::EqualTo
        );
    }

    #[test]
    fn core_compare_strings_case_insensitive2() {
        assert_eq!(
            compare_strings_case_insensitive(Some("Hello"), None),
            Comparison::GreaterThan
        );
    }

    #[test]
    fn core_compare_strings_case_insensitive3() {
        assert_eq!(
            compare_strings_case_insensitive(None, Some("hello")),
            Comparison::LessThan
        );
    }

    #[test]
    fn core_compare_strings_case_insensitive4() {
        assert_eq!(
            compare_strings_case_insensitive(None, None),
            Comparison::EqualTo
        );
    }

    #[test]
    fn core_compare_strings_case_sensitive() {
        assert_eq!(
            compare_strings_case_sensitive(Some("Hello"), Some("hello")),
            Comparison::LessThan
        );
    }

    /// `tests/data/hash_string.tsv` holds `hashString` results printed by node:
    /// fixed strings, then random ones with surrogate pairs and BMP text.
    #[test]
    fn hash_string_matches_node() {
        let data = include_str!("../../tests/data/hash_string.tsv");
        for line in data.lines() {
            let (expected, units) = line.split_once('\t').unwrap();
            let units: Vec<u16> = if units.is_empty() {
                Vec::new()
            } else {
                units
                    .split(',')
                    .map(|u| u16::from_str_radix(u, 16).unwrap())
                    .collect()
            };
            let expected: i32 = expected.parse().unwrap();
            assert_eq!(hash_string_utf16(units.iter().copied()), expected);
            if let Ok(text) = String::from_utf16(&units) {
                assert_eq!(hash_string(&text), expected);
            }
        }
    }

    #[test]
    fn string_fingerprint_preserves_the_legacy_hash() {
        for value in [
            "",
            "plain text",
            "\0",
            "\u{1F600}",
            "value_Aa = 1\n",
            "value_BB = 1\n",
        ] {
            assert_eq!(get_string_fingerprint(value).primary, hash_string(value));
        }

        let first = get_string_fingerprint("value_Aa = 1\n");
        let second = get_string_fingerprint("value_BB = 1\n");
        assert_eq!(first.primary, second.primary);
        assert_ne!(first.secondary, second.secondary);
        assert!(are_string_fingerprints_equal(&first, &first));
        assert!(!are_string_fingerprints_equal(&first, &second));
    }

    #[test]
    fn truncate_counts_utf16() {
        assert_eq!(truncate("abcdef", 5), "ab...");
        assert_eq!(truncate("abc", 5), "abc");
    }

    #[test]
    fn last_dotted_string() {
        assert_eq!(get_last_dotted_string("a.b.c"), "c");
        assert_eq!(get_last_dotted_string(".c"), ".c");
    }
}
