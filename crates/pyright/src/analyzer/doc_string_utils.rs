//! Port of `packages/pyright-internal/src/analyzer/docStringUtils.ts`.
//!
//! Static methods that format and parse doc strings based on
//! the rules specified in PEP 257
//! (https://www.python.org/dev/peps/pep-0257/).

use std::sync::LazyLock;

use jsrt::JsStr;
use jsrt::js_regex::{DOT, WHITESPACE, WORD_BOUNDARY};
use regex::Regex;

// The two `g` regexes match one literal character and their replacements hold
// no `$`, so `str::replace` with the character does what `String.replace` does.

// analyzer/docStringUtils.ts:12
const DOC_STRING_CR_REG_EX: &str = "\r";
// analyzer/docStringUtils.ts:13
const DOC_STRING_TAB_REG_EX: &str = "\t";

// analyzer/docStringUtils.ts:15
pub fn clean_and_split_doc_string(raw_string: &str) -> Vec<String> {
    // Remove carriage returns and replace tabs.
    let unescaped = raw_string
        .replace(DOC_STRING_CR_REG_EX, "")
        .replace(DOC_STRING_TAB_REG_EX, "        ");

    // Split into lines.
    let lines = unescaped.js_split("\n", None);

    // Determine the max indent amount. `i32::MAX` stands for
    // `Number.MAX_VALUE`: no line is that long.
    let mut left_spaces_to_remove = i32::MAX;
    for (index, line) in lines.iter().enumerate() {
        // First line is special.
        if lines.len() <= 1 || index > 0 {
            let trimmed = line.js_trim_start();
            if !trimmed.is_empty() {
                left_spaces_to_remove = left_spaces_to_remove.min(line.js_len() - trimmed.js_len());
            }
        }
    }

    // Handle the case where there were only empty lines.
    if left_spaces_to_remove == i32::MAX {
        left_spaces_to_remove = 0;
    }

    // Trim the lines.
    let mut trimmed_lines: Vec<String> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if index == 0 {
            trimmed_lines.push(line.js_trim().to_string());
        } else {
            trimmed_lines.push(
                line.js_substr(left_spaces_to_remove, None)
                    .js_trim_end()
                    .to_string(),
            );
        }
    }

    // Strip off leading and trailing blank lines.
    while trimmed_lines.len() > 0 && trimmed_lines[0].js_len() == 0 {
        trimmed_lines.remove(0);
    }

    while trimmed_lines.len() > 0 && trimmed_lines[trimmed_lines.len() - 1].js_len() == 0 {
        trimmed_lines.pop();
    }

    trimmed_lines
}

// analyzer/docStringUtils.ts:61
pub fn extract_parameter_documentation(
    function_doc_string: &str,
    param_name: &str,
) -> Option<String> {
    if function_doc_string.is_empty() || param_name.is_empty() {
        return None;
    }

    // Python doesn't have a single standard for param documentation. There are three
    // popular styles.
    //
    // 1. Epytext:
    //      @param param1: description
    // 2. reST:
    //      :param param1: description
    // 3. Google (variant 1):
    //      Args:
    //          param1: description
    // 4. Google (variant 2):
    //      Args:
    //          param1 (type): description

    let doc_string_lines = clean_and_split_doc_string(function_doc_string);
    for line in &doc_string_lines {
        let trimmed_line = line.js_trim();

        // Check for Epytext
        let mut param_offset = trimmed_line.js_index_of(&format!("@param {param_name}"), 0);
        if param_offset >= 0 {
            return Some(trimmed_line.js_substr(param_offset + 7, None).into_owned());
        }

        // Check for reST format
        param_offset = trimmed_line.js_index_of(&format!(":param {param_name}"), 0);
        if param_offset >= 0 {
            return Some(trimmed_line.js_substr(param_offset + 7, None).into_owned());
        }

        // Check for Google (variant 1) format
        param_offset = trimmed_line.js_index_of(&format!("{param_name}: "), 0);
        if param_offset >= 0 {
            return Some(trimmed_line.js_substr(param_offset, None).into_owned());
        }

        // Check for Google (variant 2) format
        param_offset = trimmed_line.js_index_of(&format!("{param_name} ("), 0);
        if param_offset >= 0 {
            return Some(trimmed_line.js_substr(param_offset, None).into_owned());
        }
    }

    None
}

// analyzer/docStringUtils.ts:112
pub fn extract_attribute_documentation(class_doc_string: &str, attr_name: &str) -> Option<String> {
    if class_doc_string.is_empty() || attr_name.is_empty() {
        return None;
    }

    // Python documentation styles for attributes:
    //
    // 1. reST:
    //      :ivar attr1: description
    // 2. Google:
    //      Attributes:
    //          attr1: description
    // 3. Google (with type):
    //      Attributes:
    //          attr1 (type): description

    let doc_string_lines = clean_and_split_doc_string(class_doc_string);
    for line in &doc_string_lines {
        let trimmed_line = line.js_trim();

        // Check for reST format
        let mut attr_offset = trimmed_line.js_index_of(&format!(":ivar {attr_name}"), 0);
        if attr_offset >= 0 {
            return Some(trimmed_line.js_substr(attr_offset + 6, None).into_owned());
        }

        // Check for Google (variant 1) format
        attr_offset = trimmed_line.js_index_of(&format!("{attr_name}: "), 0);
        if attr_offset >= 0 {
            return Some(trimmed_line.js_substr(attr_offset, None).into_owned());
        }

        // Check for Google (variant 2) format
        attr_offset = trimmed_line.js_index_of(&format!("{attr_name} ("), 0);
        if attr_offset >= 0 {
            return Some(trimmed_line.js_substr(attr_offset, None).into_owned());
        }
    }

    None
}

// analyzer/docStringUtils.ts:154
pub fn extract_return_documentation(function_doc_string: &str) -> Option<String> {
    // `/^:returns?:\s*(.*)$/i`
    static REST_RETURNS_REG_EX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(r"^:(?i-u:returns?):{WHITESPACE}*({DOT}*)$")).unwrap()
    });
    // `/^@returns?\b:?\s*(.*)$/i`
    static EPYTEXT_RETURNS_REG_EX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r"^@(?i-u:returns?){WORD_BOUNDARY}:?{WHITESPACE}*({DOT}*)$"
        ))
        .unwrap()
    });
    // `/^returns?:$/i`
    static GOOGLE_RETURNS_REG_EX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(?i-u:returns?):$").unwrap());

    if function_doc_string.is_empty() {
        return None;
    }

    // Python doesn't have a single standard for documenting return values. There are three
    // popular styles.
    //
    // 1. Epytext:
    //      @return: description
    //      @returns: description
    // 2. reST:
    //      :return: description
    //      :returns: description
    // 3. Google:
    //      Returns:
    //          description
    //      Returns:
    //          type: description

    // Scan the raw (only CR/tab-normalized) lines rather than cleanAndSplitDocString output.
    // cleanAndSplitDocString trims the first physical line to indent 0 and dedents the rest by a
    // common indent, which can collapse a first-line "Returns:" header and its body to the same
    // indent (e.g. `"""Returns:\n    the value."""`). The Google branch below compares header vs
    // body indentation, so it needs the original indentation preserved. reST/Epytext matching is
    // indent-agnostic and is unaffected by using raw lines.
    let normalized = function_doc_string
        .replace(DOC_STRING_CR_REG_EX, "")
        .replace(DOC_STRING_TAB_REG_EX, "        ");
    let doc_string_lines = normalized.js_split("\n", None);
    for i in 0..doc_string_lines.len() {
        let line = &doc_string_lines[i];
        let trimmed_line = line.js_trim();

        // Check for reST format (":return:" / ":returns:").
        let mut match_ = REST_RETURNS_REG_EX.captures(trimmed_line);
        if let Some(match_) = match_ {
            let description = match_[1].js_trim();
            return if description.js_len() > 0 {
                Some(description.to_string())
            } else {
                None
            };
        }

        // Check for Epytext format ("@return:" / "@returns:"). Mirror the reST handling so a
        // missing space after the colon (e.g. "@returns:foo") still extracts the description. The
        // word boundary keeps "@returnsfoo" (no delimiter) from being treated as a return field.
        match_ = EPYTEXT_RETURNS_REG_EX.captures(trimmed_line);
        if let Some(match_) = match_ {
            let description = match_[1].js_trim();
            return if description.js_len() > 0 {
                Some(description.to_string())
            } else {
                None
            };
        }

        // Check for Google format ("Returns:" / "Return:" section header). The description
        // lives on the following line(s), indented deeper than the header.
        //
        // Note: this matches the first "Returns:" line anywhere in the docstring; it does not
        // require the header to be at the outermost section indent. A bare "Returns:" nested
        // inside another section would therefore be treated as the header. That matches the
        // existing Args/parameter extraction behavior and is acceptable in practice.
        if GOOGLE_RETURNS_REG_EX.is_match(trimmed_line) {
            let header_indent = line.js_len() - line.js_trim_start().js_len();
            let mut description_lines: Vec<&str> = Vec::new();
            for j in i + 1..doc_string_lines.len() {
                let next_line = &doc_string_lines[j];
                if next_line.js_trim().js_len() == 0 {
                    // A blank line ends the section once we've started collecting.
                    if description_lines.len() > 0 {
                        break;
                    }
                    continue;
                }

                let next_indent = next_line.js_len() - next_line.js_trim_start().js_len();
                if next_indent <= header_indent {
                    // Dedented back to (or past) the header: next section started.
                    break;
                }

                description_lines.push(next_line.js_trim());
            }

            let joined = description_lines.join("\n");
            let description = joined.js_trim();
            return if description.js_len() > 0 {
                Some(description.to_string())
            } else {
                None
            };
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Port of `tests/docStringReturnExtraction.test.ts`.

    #[test]
    fn rest_returns_plural() {
        let doc = "Compute a value.\n\n:returns: The computed integer value.";
        assert_eq!(
            extract_return_documentation(doc).as_deref(),
            Some("The computed integer value.")
        );
    }

    #[test]
    fn rest_return_singular() {
        let doc = "Compute a value.\n\n:return: The computed integer value.";
        assert_eq!(
            extract_return_documentation(doc).as_deref(),
            Some("The computed integer value.")
        );
    }

    #[test]
    fn epytext_returns_plural() {
        let doc = "Compute a value.\n\n@returns: The computed integer value.";
        assert_eq!(
            extract_return_documentation(doc).as_deref(),
            Some("The computed integer value.")
        );
    }

    #[test]
    fn epytext_return_singular_no_colon() {
        let doc = "Compute a value.\n\n@return The computed integer value.";
        assert_eq!(
            extract_return_documentation(doc).as_deref(),
            Some("The computed integer value.")
        );
    }

    #[test]
    fn epytext_returns_description_no_space_after_colon() {
        // Mirrors reST: a missing space after the colon must still extract the description.
        let doc = "Compute a value.\n\n@returns:The computed integer value.";
        assert_eq!(
            extract_return_documentation(doc).as_deref(),
            Some("The computed integer value.")
        );
    }

    #[test]
    fn google_returns_single_line() {
        let doc = "Compute a value.\n\nReturns:\n    The computed integer value.";
        assert_eq!(
            extract_return_documentation(doc).as_deref(),
            Some("The computed integer value.")
        );
    }

    #[test]
    fn google_returns_multi_line_stops_at_next_section_dedent() {
        let doc = "Compute a value.\n\nReturns:\n    The computed integer value.\n    Spanning two lines.\nRaises:\n    ValueError: if the value is bad.";
        assert_eq!(
            extract_return_documentation(doc).as_deref(),
            Some("The computed integer value.\nSpanning two lines.")
        );
    }

    #[test]
    fn google_returns_multi_line_stops_at_blank_line_after_collecting() {
        let doc = "Compute a value.\n\nReturns:\n    The computed integer value.\n    Spanning two lines.\n\nSome trailing prose that is not part of the return section.";
        assert_eq!(
            extract_return_documentation(doc).as_deref(),
            Some("The computed integer value.\nSpanning two lines.")
        );
    }

    #[test]
    fn google_returns_type_description_form() {
        let doc = "Compute a value.\n\nReturns:\n    int: The computed integer value.";
        assert_eq!(
            extract_return_documentation(doc).as_deref(),
            Some("int: The computed integer value.")
        );
    }

    #[test]
    fn google_return_singular_header() {
        let doc = "Compute a value.\n\nReturn:\n    The computed integer value.";
        assert_eq!(
            extract_return_documentation(doc).as_deref(),
            Some("The computed integer value.")
        );
    }

    #[test]
    fn rest_empty_returns_yields_undefined() {
        let doc = "Compute a value.\n\n:returns:";
        assert_eq!(extract_return_documentation(doc), None);
    }

    #[test]
    fn google_returns_with_no_indented_body_yields_undefined() {
        let doc = "Compute a value.\n\nReturns:\nRaises:\n    ValueError: if the value is bad.";
        assert_eq!(extract_return_documentation(doc), None);
    }

    #[test]
    fn no_return_section_yields_undefined() {
        let doc = "Compute a value.\n\nArgs:\n    x: the input.";
        assert_eq!(extract_return_documentation(doc), None);
    }

    #[test]
    fn google_returns_header_on_the_first_physical_line() {
        // The "Returns:" header sits on the docstring's first physical line, so its indentation
        // must still be recovered correctly even though the first line is special-cased during
        // docstring cleanup.
        let doc = "Returns:\n    The computed integer value.";
        assert_eq!(
            extract_return_documentation(doc).as_deref(),
            Some("The computed integer value.")
        );
    }

    #[test]
    fn google_returns_header_on_the_first_physical_line_multi_line_body() {
        let doc = "Returns:\n    The computed integer value.\n    Spanning two lines.";
        assert_eq!(
            extract_return_documentation(doc).as_deref(),
            Some("The computed integer value.\nSpanning two lines.")
        );
    }

    #[test]
    fn empty_docstring_yields_undefined() {
        assert_eq!(extract_return_documentation(""), None);
    }

    // JS string and regex semantics. Expected values come from node running
    // `docStringUtils.ts`.

    #[test]
    fn js_trim_keeps_nel_and_strips_bom_and_line_separators() {
        assert_eq!(
            clean_and_split_doc_string("\u{85}x\u{85}\n\u{a0} a\u{85}\n\u{feff}\t b\r\n\u{2028}"),
            vec!["\u{85}x\u{85}", "a\u{85}", "        b"]
        );
        assert_eq!(
            extract_return_documentation("\u{a0}Returns:\n\u{feff}  y\n z").as_deref(),
            Some("y")
        );
    }

    #[test]
    fn utf16_offsets() {
        assert_eq!(
            clean_and_split_doc_string("\n  \u{1F600}a\n    b\n"),
            vec!["\u{1F600}a", "  b"]
        );
        assert_eq!(
            extract_parameter_documentation("\u{1F600}@param a: b", "a").as_deref(),
            Some("a: b")
        );
    }

    #[test]
    fn js_regex_semantics() {
        // `\b` is ASCII: `ſ` and `é` are not word characters.
        assert_eq!(
            extract_return_documentation("@return\u{17f}: x").as_deref(),
            Some("\u{17f}: x")
        );
        assert_eq!(
            extract_return_documentation("@returns\u{e9} x").as_deref(),
            Some("\u{e9} x")
        );
        // `\s` matches U+2028.
        assert_eq!(
            extract_return_documentation(":returns:\u{2028}x").as_deref(),
            Some("x")
        );
        // `/i` without `u` folds ASCII letters only: `ſ` does not match `s`.
        assert_eq!(extract_return_documentation("return\u{17f}:\n  x"), None);
        assert_eq!(extract_return_documentation(":return\u{17f}: x"), None);
        assert_eq!(
            extract_return_documentation("RETURNS:\n  x").as_deref(),
            Some("x")
        );
    }
}
