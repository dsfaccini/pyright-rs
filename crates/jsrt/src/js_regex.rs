//! JS regex escapes spelled for the `regex` crate, where the two differ.
//!
//! Pyright's regexes have no `u` flag. In that mode `\s` is the JS whitespace
//! set, `\w`, `\d` and `\b` are ASCII, and `.` stops at four line terminators.
//! The `regex` crate's `\s`, `\w`, `\d` and `\b` are Unicode, and its `.` only
//! stops at `\n`. Splice these fragments into a `regex` pattern in place of
//! the JS escape.
//!
//! For `/i` on a pattern whose letters are all ASCII, wrap those letters in
//! `(?i-u:...)`: a non-`u` JS regex never folds a non-ASCII character to an
//! ASCII one, so `/k/i` does not match the Kelvin sign and `/s/i` does not
//! match `ſ`. The `regex` crate's `(?i)` matches both.
//!
//! Not expressible here: a non-`u` JS regex sees an astral character as two
//! code units, so `.` or `[^/]` matches half of it, and the `regex` crate has
//! no lookaround. Port those patterns by hand.

/// JS `\s`.
pub const WHITESPACE: &str = r"[\t\n\x0B\x0C\r \xA0\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";

/// JS `\S`.
pub const NOT_WHITESPACE: &str = r"[^\t\n\x0B\x0C\r \xA0\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";

/// JS `\s` inside a character class: `[^\w\s/]` is
/// `format!("[^{WORD_IN_CLASS}{WHITESPACE_IN_CLASS}/]")`.
pub const WHITESPACE_IN_CLASS: &str =
    r"\t\n\x0B\x0C\r \xA0\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";

/// JS `\w`.
pub const WORD: &str = "[0-9A-Za-z_]";

/// JS `\W`.
pub const NOT_WORD: &str = "[^0-9A-Za-z_]";

/// JS `\w` inside a character class.
pub const WORD_IN_CLASS: &str = "0-9A-Za-z_";

/// JS `\d`.
pub const DIGIT: &str = "[0-9]";

/// JS `\D`.
pub const NOT_DIGIT: &str = "[^0-9]";

/// JS `\b`: a boundary between an ASCII word character and anything else.
pub const WORD_BOUNDARY: &str = r"(?-u:\b)";

/// JS `.` without the `s` flag.
pub const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";
