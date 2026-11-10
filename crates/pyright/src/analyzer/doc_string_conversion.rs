//! Port of `packages/pyright-internal/src/analyzer/docStringConversion.ts`, the part the check
//! path reaches: `convertDocStringToPlainText`, which `getDeprecatedMessageFromCall`
//! (decorators.ts) applies to a `@deprecated("...")` message. The Markdown conversion serves
//! the language server only.

use crate::analyzer::doc_string_utils::clean_and_split_doc_string;
use jsrt::JsStr;
use jsrt::string::is_js_whitespace_char;

// analyzer/docStringConversion.ts:29
//  Converts a docstring to a plaintext, human readable form. This will
//  first strip any common leading indention (like inspect.cleandoc),
//  then remove duplicate empty/whitespace lines.
pub fn convert_doc_string_to_plain_text(doc_string: &str) -> String {
    let lines = clean_and_split_doc_string(doc_string);
    let mut output: Vec<String> = Vec::new();

    for line in lines {
        let last = output.last().map(|last| last.as_str());
        if _is_undefined_or_whitespace(Some(&line)) && _is_undefined_or_whitespace(last) {
            continue;
        }

        output.push(line);
    }

    output.join("\n").js_trim_end().to_string()
}

// analyzer/docStringConversion.ts:866
/// `!/\S/.test(s)`: JavaScript's whitespace set.
fn _is_undefined_or_whitespace(s: Option<&str>) -> bool {
    s.is_none_or(|s| s.chars().all(is_js_whitespace_char))
}
