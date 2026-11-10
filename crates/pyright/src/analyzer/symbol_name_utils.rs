//! Port of `packages/pyright-internal/src/analyzer/symbolNameUtils.ts`.
//!
//! Static methods that apply to symbols or symbol names.
//!
//! The regexes hold only ASCII classes, so the `regex` crate matches them as
//! a non-`u` JS regex does.

use std::sync::LazyLock;

use jsrt::JsStr;
use regex::Regex;

// analyzer/symbolNameUtils.ts:10
static _CONSTANT_REG_EX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z0-9_]+$").unwrap());
// analyzer/symbolNameUtils.ts:11
static _UNDERSCORE_ONLY_REG_EX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[_]+$").unwrap());
// analyzer/symbolNameUtils.ts:12
static _CAMEL_CASE_REG_EX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^_{0,2}[A-Z][A-Za-z0-9_]+$").unwrap());

// analyzer/symbolNameUtils.ts:15
// Private symbol names start with a double underscore.
pub fn is_private_name(name: &str) -> bool {
    name.js_len() > 2 && name.starts_with("__") && !name.ends_with("__")
}

// analyzer/symbolNameUtils.ts:20
// Protected symbol names start with a single underscore.
pub fn is_protected_name(name: &str) -> bool {
    name.js_len() > 1 && name.starts_with('_') && !name.starts_with("__")
}

// analyzer/symbolNameUtils.ts:24
pub fn is_private_or_protected_name(name: &str) -> bool {
    is_private_name(name) || is_protected_name(name)
}

// analyzer/symbolNameUtils.ts:29
// "Dunder" names start and end with two underscores.
pub fn is_dunder_name(name: &str) -> bool {
    name.js_len() > 4 && name.starts_with("__") && name.ends_with("__")
}

// analyzer/symbolNameUtils.ts:34
// "Single Dunder" names start and end with single underscores.
pub fn is_single_dunder_name(name: &str) -> bool {
    name.js_len() > 2 && name.starts_with('_') && name.ends_with('_')
}

// analyzer/symbolNameUtils.ts:39
// Constants are all-caps with possible numbers and underscores.
pub fn is_constant_name(name: &str) -> bool {
    _CONSTANT_REG_EX.is_match(name) && !_UNDERSCORE_ONLY_REG_EX.is_match(name)
}

// analyzer/symbolNameUtils.ts:44
// Type aliases are CamelCase with possible numbers and underscores.
pub fn is_type_alias_name(name: &str) -> bool {
    _CAMEL_CASE_REG_EX.is_match(name)
}

// analyzer/symbolNameUtils.ts:48
pub fn is_public_constant_or_type_alias(name: &str) -> bool {
    !is_private_or_protected_name(name) && (is_constant_name(name) || is_type_alias_name(name))
}

// Port of `tests/symbolNameUtils.test.ts`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_name_utils_is_private_name() {
        assert!(is_private_name("__var"));
        assert!(is_private_name("__Var_1-2"));

        assert!(!is_private_name("var"));
        assert!(!is_private_name("_var"));
        assert!(!is_private_name("__var__"));
    }

    #[test]
    fn symbol_name_utils_is_protected_name() {
        assert!(is_protected_name("_var"));
        assert!(is_protected_name("_Var_1-2"));

        assert!(!is_protected_name("__var"));
        assert!(!is_protected_name("var"));
    }

    #[test]
    fn symbol_name_utils_is_private_or_protected_name() {
        assert!(is_private_or_protected_name("_var"));
        assert!(is_private_or_protected_name("__VAR_1-2"));

        assert!(!is_private_or_protected_name("var"));
        assert!(!is_private_or_protected_name("__init__"));
    }

    #[test]
    fn symbol_name_utils_is_dunder_name() {
        assert!(is_dunder_name("__init__"));
        assert!(is_dunder_name("__CONSTANT__"));

        assert!(!is_dunder_name("____"));
        assert!(!is_dunder_name("_init_"));
        assert!(!is_dunder_name("init"));
    }

    #[test]
    fn symbol_name_utils_is_constant_name() {
        assert!(is_constant_name("CONSTANT"));
        assert!(is_constant_name("CONSTANT_NAME"));
        assert!(is_constant_name("CONSTANT_42"));
        assert!(is_constant_name("_CONSTANT_42"));
        assert!(is_constant_name("__CONSTANT_42"));

        assert!(!is_constant_name("Constant"));
        assert!(!is_constant_name("constant"));
        assert!(!is_constant_name("____"));
    }

    #[test]
    fn symbol_name_utils_is_type_alias_name() {
        assert!(is_type_alias_name("TypeAlias"));
        assert!(is_type_alias_name("Type_alias"));
        assert!(is_type_alias_name("TypeAlias1"));
        assert!(is_type_alias_name("_TypeAlias"));
        assert!(is_type_alias_name("__TypeAlias"));

        assert!(!is_type_alias_name("invalidTypeAlias"));
        assert!(!is_type_alias_name("1TypeAlias"));
        assert!(!is_type_alias_name("___TypeAlias"));
    }

    #[test]
    fn symbol_name_utils_is_public_constant_or_type_alias_name() {
        assert!(is_public_constant_or_type_alias("CONSTANT"));
        assert!(is_public_constant_or_type_alias("TypeAlias"));

        assert!(!is_public_constant_or_type_alias("var"));
        assert!(!is_public_constant_or_type_alias("_CONSTANT"));
        assert!(!is_public_constant_or_type_alias("_TypeAlias"));
        assert!(!is_public_constant_or_type_alias("__TypeAlias"));
    }
}
