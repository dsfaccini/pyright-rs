//! Port of `packages/pyright-internal/src/common/core.ts`.
//!
//! Only the helpers the check path calls. JS type predicates (`isString`,
//! `isArray`, `isThenable`, ...) have no Rust counterpart: the type system
//! answers them.

use jsrt::JsStr;
use std::cmp::Ordering;

// common/core.ts:9
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    LessThan = -1,
    EqualTo = 0,
    GreaterThan = 1,
}

impl Comparison {
    pub fn from_ordering(ordering: Ordering) -> Comparison {
        match ordering {
            Ordering::Less => Comparison::LessThan,
            Ordering::Equal => Comparison::EqualTo,
            Ordering::Greater => Comparison::GreaterThan,
        }
    }

    pub fn to_ordering(self) -> Ordering {
        match self {
            Comparison::LessThan => Ordering::Less,
            Comparison::EqualTo => Ordering::Equal,
            Comparison::GreaterThan => Ordering::Greater,
        }
    }
}

// common/core.ts:46
pub fn to_lower_case(x: &str) -> String {
    x.js_to_lower_case()
}

// common/core.ts:54
/// The string overload: `<` on JS strings compares UTF-16 code units.
pub fn compare_comparable_values_str(a: Option<&str>, b: Option<&str>) -> Comparison {
    match (a, b) {
        _ if a == b => Comparison::EqualTo,
        (None, _) => Comparison::LessThan,
        (_, None) => Comparison::GreaterThan,
        (Some(a), Some(b)) if a.js_cmp(b) == Ordering::Less => Comparison::LessThan,
        _ => Comparison::GreaterThan,
    }
}

// common/core.ts:54
/// The number overload.
pub fn compare_comparable_values_num(a: Option<f64>, b: Option<f64>) -> Comparison {
    match (a, b) {
        _ if a == b => Comparison::EqualTo,
        (None, _) => Comparison::LessThan,
        (_, None) => Comparison::GreaterThan,
        (Some(a), Some(b)) if a < b => Comparison::LessThan,
        _ => Comparison::GreaterThan,
    }
}

// common/core.ts:73
pub fn compare_values(a: Option<f64>, b: Option<f64>) -> Comparison {
    compare_comparable_values_num(a, b)
}

// common/core.ts:118
pub fn to_boolean(true_or_false: &str) -> bool {
    let normalized = true_or_false.js_trim().js_to_upper_case();
    if normalized == "TRUE" {
        return true;
    }

    false
}

// common/core.ts:134
/// The CLI never runs under a Node inspector.
pub fn is_debug_mode() -> bool {
    false
}

// common/core.ts:174
pub fn contains_only_whitespace(text: &str, start: Option<i32>, end: Option<i32>) -> bool {
    let text = match start {
        Some(start) => text.js_substring(start, end).into_owned(),
        None => text.to_string(),
    };

    text.chars().all(is_js_whitespace_char)
}

/// JS regex `\s` for one character. Every JS whitespace character is in the BMP.
pub fn is_js_whitespace_char(c: char) -> bool {
    (c as u32) <= 0xFFFF && jsrt::string::is_js_whitespace(c as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_strings_in_utf16_order() {
        // U+FF5E sorts after U+1F600 in UTF-16 (0xFF5E > 0xD83D) but before it in UTF-8.
        assert_eq!(
            compare_comparable_values_str(Some("\u{1F600}"), Some("\u{FF5E}")),
            Comparison::LessThan
        );
        assert_eq!(
            compare_comparable_values_str(None, Some("a")),
            Comparison::LessThan
        );
        assert_eq!(
            compare_comparable_values_str(Some("a"), Some("a")),
            Comparison::EqualTo
        );
    }

    #[test]
    fn to_boolean_trims_and_uppercases() {
        assert!(to_boolean(" true "));
        assert!(to_boolean("TRUE"));
        assert!(!to_boolean("yes"));
    }
}
