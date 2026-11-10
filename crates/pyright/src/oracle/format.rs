//! Value formatting shared by the oracle dump stages (`oracle/FORMAT.md`, "values").
//!
//! - `DumpWriter` collects the lines of one dump file: `field` writes
//!   `<indent><key>: <value>`, `open` writes `<indent><key>:` (a nested object
//!   whose fields follow at `depth + 1`). `finish` returns the file text, every
//!   line ending in `\n`.
//! - Scalars: `string` (UTF-16, JSON.stringify escape), `str` (the same for a
//!   Rust `&str`), `number` (JS `String(n)`), `number_value` (number, or bigint
//!   digits plus `n`), `boolean`.
//! - One-line forms: `text_range` (`<start>+<length>`).
//! - Containers: `array_header` (`[<length>]`), `map_header` (`Map(<size>)`),
//!   `index_key` (`[<i>]`).
//! - `map_path` maps a file path to its dump path.

use jsrt::number::number_to_string;
use jsrt::{JsStr, NumberValue};

use crate::common::text_range::HasTextRange;

/// The lines of one dump file.
#[derive(Default)]
pub struct DumpWriter {
    out: String,
}

impl DumpWriter {
    pub fn new() -> DumpWriter {
        DumpWriter::default()
    }

    /// `<indent><key>: <value>`.
    pub fn field(&mut self, depth: usize, key: &str, value: &str) {
        self.indent(depth);
        self.out.push_str(key);
        self.out.push_str(": ");
        self.out.push_str(value);
        self.out.push('\n');
    }

    /// `<indent><key>:`, the header of a nested object.
    pub fn open(&mut self, depth: usize, key: &str) {
        self.indent(depth);
        self.out.push_str(key);
        self.out.push_str(":\n");
    }

    pub fn finish(self) -> String {
        self.out
    }

    fn indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.out.push_str("  ");
        }
    }
}

/// A JS string, escaped and quoted as `JSON.stringify` does.
pub fn string(units: &[u16]) -> String {
    units.js_json_stringify()
}

/// A Rust string, escaped and quoted as `JSON.stringify` does.
pub fn str(text: &str) -> String {
    text.js_json_stringify()
}

/// JS `String(n)`.
pub fn number(n: f64) -> String {
    number_to_string(n)
}

/// A `number | bigint`: `String(n)` for a number, the digits plus `n` for a bigint.
pub fn number_value(value: &NumberValue) -> String {
    match value {
        NumberValue::Number(n) => number_to_string(*n),
        NumberValue::BigInt(b) => format!("{}n", b.to_js_string()),
    }
}

pub fn boolean(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

/// The one-line form of a `TextRange`: `<start>+<length>`.
pub fn text_range(range: &(impl HasTextRange + ?Sized)) -> String {
    let range = range.text_range();
    format!("{}+{}", range.start, range.length)
}

/// An array or `TextRangeCollection` header: `[<length>]`.
pub fn array_header(length: usize) -> String {
    format!("[{length}]")
}

/// A `Map` header: `Map(<size>)`.
pub fn map_header(size: usize) -> String {
    format!("Map({size})")
}

/// An array element key: `[<i>]`.
pub fn index_key(i: usize) -> String {
    format!("[{i}]")
}

/// The dump path of a file path: `typeshed/...` for a file in a bundled
/// typeshed, `fs/...` for an absolute path, `other/...` otherwise.
pub fn map_path(path: &str) -> String {
    const TYPESHED: &str = "/typeshed-fallback/";
    if let Some(i) = path.find(TYPESHED) {
        return format!("typeshed/{}", &path[i + TYPESHED.len()..]);
    }
    if path.starts_with('/') {
        return format!("fs{path}");
    }
    format!("other/{path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_map_like_the_fork() {
        assert_eq!(
            map_path("/x/dist/typeshed-fallback/stdlib/builtins.pyi"),
            "typeshed/stdlib/builtins.pyi"
        );
        assert_eq!(map_path("/home/a/x.py"), "fs/home/a/x.py");
        assert_eq!(map_path("x.py"), "other/x.py");
    }

    #[test]
    fn strings_escape_like_json_stringify() {
        assert_eq!(str("a\"b\\\n\u{1}"), "\"a\\\"b\\\\\\n\\u0001\"");
        assert_eq!(string(&[0xd800]), "\"\\ud800\"");
    }
}
