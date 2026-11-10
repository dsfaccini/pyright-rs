//! Port of `packages/pyright-internal/src/common/uri/uriUtils.ts`.

use regex::{Regex, RegexBuilder};

use crate::common::path_utils::{
    get_regex_escaped_separator, is_directory_wildcard_pattern_present,
    strip_trailing_directory_separator,
};
use crate::common::uri::Uri;

// common/uri/uriUtils.ts:22
#[derive(Clone, Debug)]
pub struct FileSpec {
    pub wildcard_root: Uri,
    pub reg_exp: Regex,
    pub has_directory_wildcard: bool,
}

impl FileSpec {
    // common/uri/uriUtils.ts:37
    pub fn is_in_path(uri: Uri, paths: &[FileSpec]) -> bool {
        paths.iter().any(|p| uri.matches_regex(&p.reg_exp))
    }

    // common/uri/uriUtils.ts:41
    pub fn matches_include_file_regex(uri: Uri, is_file: bool) -> bool {
        if is_file {
            uri.matches_regex(include_file_regex())
        } else {
            true
        }
    }

    // common/uri/uriUtils.ts:45
    pub fn match_include_file_spec(
        include_reg_exp: &Regex,
        exclude: &[FileSpec],
        uri: Uri,
        is_file: bool,
    ) -> bool {
        if uri.matches_regex(include_reg_exp) {
            if !FileSpec::is_in_path(uri, exclude)
                && FileSpec::matches_include_file_regex(uri, is_file)
            {
                return true;
            }
        }

        false
    }
}

// common/uri/uriUtils.ts:29
fn include_file_regex() -> &'static Regex {
    thread_local! {
        static RE: &'static Regex = Box::leak(Box::new(Regex::new(r"\.pyi?$").unwrap()));
    }
    RE.with(|r| *r)
}

// common/uri/uriUtils.ts:62
pub fn for_each_ancestor_directory(
    directory: Uri,
    mut callback: impl FnMut(Uri) -> Option<Uri>,
) -> Option<Uri> {
    let mut directory = directory;
    loop {
        if let Some(result) = callback(directory) {
            return Some(result);
        }

        let parent_path = directory.get_directory();
        if parent_path.equals(Some(directory)) {
            return None;
        }

        directory = parent_path;
    }
}

// common/uri/uriUtils.ts:257
pub fn contains_wildcard_character(text: &str) -> bool {
    text.contains(['*', '?'])
}

// common/uri/uriUtils.ts:261
/// Builds the fragment in `regex` crate syntax. JS escapes every character
/// outside `[\w\s/]` with a backslash; the `regex` crate accepts a backslash
/// before ASCII punctuation only, and needs none before other characters.
pub fn get_wildcard_segment_regex_fragment(segment: &str) -> String {
    let escaped_separator = get_regex_escaped_separator();
    let mut out = String::new();
    for c in segment.chars() {
        let is_word = c.is_ascii_alphanumeric() || c == '_';
        if is_word || crate::common::core::is_js_whitespace_char(c) || c == '/' {
            out.push(c);
        } else if c == '*' {
            out.push_str(&format!("[^{escaped_separator}]*"));
        } else if c == '?' {
            out.push_str(&format!("[^{escaped_separator}]"));
        } else if c.is_ascii_punctuation() {
            out.push('\\');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

// common/uri/uriUtils.ts:276
pub fn get_wildcard_regex_pattern(root: Uri, file_spec: &str) -> String {
    let absolute_path = root.resolve_paths(&[file_spec]);
    let mut path_components = absolute_path.get_path_components();
    let escaped_separator = get_regex_escaped_separator();
    let double_asterisk_regex_fragment =
        format!("({escaped_separator}[^{escaped_separator}][^{escaped_separator}]*)*?");

    if !path_components.is_empty() {
        path_components[0] = strip_trailing_directory_separator(&path_components[0]).to_string();
    }

    let mut reg_ex_pattern = String::new();
    let mut first_component = true;

    for component in &path_components {
        if component == "**" {
            reg_ex_pattern.push_str(&double_asterisk_regex_fragment);
        } else {
            if !first_component {
                reg_ex_pattern.push_str(escaped_separator);
            }

            reg_ex_pattern.push_str(&get_wildcard_segment_regex_fragment(component));
            first_component = false;
        }
    }

    reg_ex_pattern
}

// common/uri/uriUtils.ts:305
pub fn get_wildcard_root(root: Uri, file_spec: &str) -> Uri {
    let absolute_path = root.resolve_paths(&[file_spec]);
    let mut path_components = absolute_path.get_path_components();
    let mut wildcard_root = absolute_path.root();

    if !path_components.is_empty() {
        path_components.remove(0);
    }

    for component in &path_components {
        if component == "**" {
            break;
        } else {
            if contains_wildcard_character(component) {
                break;
            }

            wildcard_root = wildcard_root.resolve_paths(&[component]);
        }
    }

    wildcard_root
}

// common/uri/uriUtils.ts:330
pub fn has_python_extension(uri: Uri) -> bool {
    uri.has_extension(".py") || uri.has_extension(".pyi")
}

// common/uri/uriUtils.ts:334
pub fn get_file_spec(root: Uri, file_spec: &str) -> FileSpec {
    let reg_ex_pattern = get_wildcard_regex_pattern(root, file_spec);
    let escaped_separator = get_regex_escaped_separator();
    let reg_ex_pattern = format!("^({reg_ex_pattern})($|{escaped_separator})");

    let reg_exp = RegexBuilder::new(&reg_ex_pattern)
        .case_insensitive(!root.is_case_sensitive())
        .build()
        .unwrap_or_else(|e| panic!("file spec regex {reg_ex_pattern}: {e}"));
    let wildcard_root = get_wildcard_root(root, file_spec);
    let has_directory_wildcard = is_directory_wildcard_pattern_present(file_spec);

    FileSpec {
        wildcard_root,
        reg_exp,
        has_directory_wildcard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::case_sensitivity_detector::CaseSensitiveDetector;

    fn file(path: &str) -> Uri {
        Uri::file(path, &CaseSensitiveDetector, false)
    }

    #[test]
    fn file_spec_matches_like_pyright() {
        let root = file("/proj");
        let spec = get_file_spec(root, "src/**/*.py");
        assert_eq!(spec.wildcard_root.get_file_path(), "/proj/src");
        assert!(spec.has_directory_wildcard);
        assert!(file("/proj/src/a/b/c.py").matches_regex(&spec.reg_exp));
        assert!(file("/proj/src/c.py").matches_regex(&spec.reg_exp));
        assert!(!file("/proj/other/c.py").matches_regex(&spec.reg_exp));

        let exclude = get_file_spec(root, "**/node_modules");
        assert!(file("/proj/a/node_modules").matches_regex(&exclude.reg_exp));
        assert!(file("/proj/a/node_modules/x.py").matches_regex(&exclude.reg_exp));
        assert!(!file("/proj/a/node_modules2").matches_regex(&exclude.reg_exp));

        let dot = get_file_spec(root, "**/.*");
        assert!(file("/proj/.venv").matches_regex(&dot.reg_exp));
        assert!(!file("/proj/venv").matches_regex(&dot.reg_exp));
    }

    #[test]
    fn punctuation_and_unicode_segments() {
        let root = file("/proj");
        let spec = get_file_spec(root, "a+b/é~(x)");
        assert!(file("/proj/a+b/é~(x)/y.py").matches_regex(&spec.reg_exp));
        assert!(!file("/proj/aab/é~(x)").matches_regex(&spec.reg_exp));
    }

    #[test]
    fn ancestors() {
        let mut seen = Vec::new();
        for_each_ancestor_directory(file("/a/b"), |d| {
            seen.push(d.get_file_path().to_string());
            None
        });
        assert_eq!(seen, ["/a/b", "/a", "/"]);
    }
}
