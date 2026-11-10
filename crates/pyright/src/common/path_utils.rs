//! Port of `packages/pyright-internal/src/common/pathUtils.ts`.
//!
//! POSIX host only: `path.sep` is `/`. Separators and dots are ASCII, so byte
//! offsets equal UTF-16 offsets at every slice point below.

use jsrt::JsStr;
use jsrt::node_path;
use regex::Regex;

use crate::common::string_utils::{equate_strings_case_insensitive, equate_strings_case_sensitive};

const SEP: char = '/';

// common/pathUtils.ts:20
#[derive(Clone, Debug)]
pub struct FileSpec {
    /// The original filespec, as written in the config (`wildcardRoot` is a Uri
    /// in `uriUtils.getFileSpec`; this one is the string form).
    pub wildcard_root: String,
    pub reg_exp: Regex,
    pub has_directory_wildcard: bool,
}

// common/pathUtils.ts:73
pub fn get_directory_path(path_string: &str) -> String {
    let last = path_string.rfind(SEP).map_or(-1, |i| i as isize);
    let end = (get_root_length(path_string) as isize).max(last) as usize;
    path_string[..end.min(path_string.len())].to_string()
}

// common/pathUtils.ts:80
pub fn get_root_length(path_string: &str) -> usize {
    let b = path_string.as_bytes();
    if b.first() == Some(&(SEP as u8)) {
        if b.get(1) != Some(&(SEP as u8)) {
            return 1;
        }
        return match path_string[2..].find(SEP) {
            None => path_string.len(),
            Some(p1) => p1 + 2 + 1,
        };
    }
    if b.get(1) == Some(&b':') {
        if b.get(2) == Some(&(SEP as u8)) {
            return 3;
        }
        if b.len() == 2 {
            return 2;
        }
    }

    0
}

// common/pathUtils.ts:104
pub fn get_path_separator(_path_string: &str) -> &'static str {
    "/"
}

// common/pathUtils.ts:108
pub fn get_path_components(path_string: &str) -> Vec<String> {
    let normalized_path = normalize_slashes(path_string);
    let root_length = get_root_length(&normalized_path);
    let root = normalized_path[..root_length].to_string();
    let mut rest: Vec<String> = normalized_path[root_length..]
        .split(SEP)
        .map(str::to_string)
        .collect();
    if !rest.is_empty() && rest.last().unwrap().is_empty() {
        rest.pop();
    }

    let mut components = vec![root];
    components.extend(rest);
    reduce_path_components(&components)
}

// common/pathUtils.ts:122
pub fn reduce_path_components(components: &[String]) -> Vec<String> {
    if components.is_empty() {
        return Vec::new();
    }

    let mut reduced = vec![components[0].clone()];
    for component in &components[1..] {
        if component.is_empty() || component == "." {
            continue;
        }

        if component == ".." {
            if reduced.len() > 1 {
                if reduced.last().unwrap() != ".." {
                    reduced.pop();
                    continue;
                }
            } else if !reduced[0].is_empty() {
                continue;
            }
        }
        reduced.push(component.clone());
    }

    reduced
}

// common/pathUtils.ts:151
pub fn combine_path_components(components: &[String]) -> String {
    if components.is_empty() {
        return String::new();
    }

    let root = if components[0].is_empty() {
        String::new()
    } else {
        ensure_trailing_directory_separator(&components[0])
    };
    normalize_slashes(&(root + &components[1..].join("/")))
}

// common/pathUtils.ts:161
pub fn get_relative_path(dir_path: &str, relative_to: &str) -> Option<String> {
    if !dir_path.starts_with(&ensure_trailing_directory_separator(relative_to)) {
        return None;
    }

    let path_components = get_path_components(dir_path);
    let relative_to_components = get_path_components(relative_to);

    let mut relative_path = ".".to_string();
    for component in &path_components[relative_to_components.len().min(path_components.len())..] {
        relative_path.push(SEP);
        relative_path.push_str(component);
    }

    Some(relative_path)
}

// common/pathUtils.ts:180
pub fn normalize_slashes(path_string: &str) -> String {
    if path_string.contains('\\') {
        return path_string.replace('\\', "/");
    }

    path_string.to_string()
}

// common/pathUtils.ts:199
pub fn resolve_paths(path: &str, paths: &[&str]) -> String {
    normalize_path(&if !paths.is_empty() {
        combine_paths(path, paths)
    } else {
        normalize_slashes(path)
    })
}

// common/pathUtils.ts:203
pub fn combine_paths(path_string: &str, paths: &[&str]) -> String {
    let mut path_string = if !path_string.is_empty() {
        normalize_slashes(path_string)
    } else {
        String::new()
    };

    for relative_path in paths {
        if relative_path.is_empty() {
            continue;
        }

        let relative_path = normalize_slashes(relative_path);

        if path_string.is_empty() || get_root_length(&relative_path) != 0 {
            path_string = relative_path;
        } else {
            path_string = ensure_trailing_directory_separator(&path_string) + &relative_path;
        }
    }

    path_string
}

// common/pathUtils.ts:230
pub fn contains_path(parent: &str, child: &str, ignore_case: bool) -> bool {
    if parent == child {
        return true;
    }

    let parent_components = get_path_components(parent);
    let child_components = get_path_components(child);

    if child_components.len() < parent_components.len() {
        return false;
    }

    let component_equality_comparer: fn(&str, &str) -> bool = if ignore_case {
        equate_strings_case_insensitive
    } else {
        equate_strings_case_sensitive
    };
    for i in 0..parent_components.len() {
        let equality_comparer = if i == 0 {
            equate_strings_case_insensitive
        } else {
            component_equality_comparer
        };
        if !equality_comparer(&parent_components[i], &child_components[i]) {
            return false;
        }
    }

    true
}

// common/pathUtils.ts:279
pub fn change_any_extension(path: &str, ext: &str) -> String {
    let path_ext = get_any_extension_from_path(path);
    if path_ext.is_empty() {
        return path.to_string();
    }
    let base = &path[..path.len() - path_ext.len()];
    if ext.starts_with('.') {
        format!("{base}{ext}")
    } else {
        format!("{base}.{ext}")
    }
}

// common/pathUtils.ts:319
pub fn get_any_extension_from_path(path: &str) -> String {
    let base_file_name = get_base_file_name(path);
    match base_file_name.rfind('.') {
        Some(extension_index) => base_file_name[extension_index..].to_string(),
        None => String::new(),
    }
}

// common/pathUtils.ts:371
pub fn get_base_file_name(path_string: &str) -> String {
    let path_string = normalize_slashes(path_string);

    let root_length = get_root_length(&path_string);
    if root_length == path_string.len() {
        return String::new();
    }

    let path_string = strip_trailing_directory_separator(&path_string);
    let last = path_string.rfind(SEP).map_or(0, |i| i + 1);
    let start = get_root_length(path_string).max(last);
    path_string[start..].to_string()
}

// common/pathUtils.ts:427
pub fn ensure_trailing_directory_separator(path_string: &str) -> String {
    if !has_trailing_directory_separator(path_string) {
        return format!("{path_string}/");
    }

    path_string.to_string()
}

// common/pathUtils.ts:436
pub fn has_trailing_directory_separator(path_string: &str) -> bool {
    matches!(path_string.as_bytes().last(), Some(b'/' | b'\\'))
}

// common/pathUtils.ts:445
pub fn strip_trailing_directory_separator(path_string: &str) -> &str {
    if !has_trailing_directory_separator(path_string) {
        return path_string;
    }
    &path_string[..path_string.len() - 1]
}

// common/pathUtils.ts:452
pub fn get_file_extension(file_name: &str, multi_dot_extension: bool) -> String {
    if !multi_dot_extension {
        return node_path::extname(file_name).to_string();
    }

    let file_name = get_file_name(file_name);
    // `indexOf` returns -1 when there is no dot, and `slice(-1)` keeps the
    // last code unit.
    match file_name.find('.') {
        Some(first_dot_index) => file_name[first_dot_index..].to_string(),
        None => file_name.js_slice(-1, None).into_owned(),
    }
}

// common/pathUtils.ts:462
pub fn get_file_name(path_string: &str) -> &str {
    node_path::basename(path_string)
}

// common/pathUtils.ts:466
pub fn get_shortened_file_name(path_string: &str, max_dir_length: i32) -> String {
    let file_name = get_file_name(path_string);
    let dir_name = get_directory_path(path_string);
    let dir_len = dir_name.js_len();
    if dir_len > max_dir_length {
        let tail = dir_name.js_slice(dir_len - max_dir_length, None);
        return format!("...{tail}/{file_name}");
    }
    path_string.to_string()
}

// common/pathUtils.ts:475
pub fn strip_file_extension(file_name: &str, multi_dot_extension: bool) -> String {
    let ext = get_file_extension(file_name, multi_dot_extension);
    file_name
        .js_substr(0, file_name.js_len() - ext.js_len())
        .into_owned()
}

// common/pathUtils.ts:480
pub fn normalize_path(path_string: &str) -> String {
    normalize_slashes(&node_path::normalize(path_string))
}

// common/pathUtils.ts:484
/// Returns the pattern as pyright builds it, as JS regex source, and the
/// equivalent pattern in `regex` crate syntax.
pub fn get_wildcard_regex_pattern(root_path: &str, file_spec: &str) -> String {
    let mut absolute_path = normalize_path(&combine_paths(root_path, &[file_spec]));
    if !has_python_extension(&absolute_path) {
        absolute_path = ensure_trailing_directory_separator(&absolute_path);
    }

    let mut path_components = get_path_components(&absolute_path);

    let escaped_separator = get_regex_escaped_separator();
    let double_asterisk_regex_fragment =
        format!("({escaped_separator}[^{escaped_separator}][^{escaped_separator}]*)*?");

    if !path_components.is_empty() {
        path_components[0] = strip_trailing_directory_separator(&path_components[0]).to_string();

        if path_components[0].starts_with("\\\\") {
            path_components[0] = format!("\\\\{}", path_components[0]);
        }
    }

    let mut reg_ex_pattern = String::new();
    let mut first_component = true;

    for component in &path_components {
        if component == "**" {
            reg_ex_pattern.push_str(&double_asterisk_regex_fragment);
        } else {
            let component = if !first_component {
                format!("{escaped_separator}{component}")
            } else {
                component.clone()
            };

            // `component.replace(/[^\w\s<sep>]/g, ...)`: JS `\w` is ASCII only.
            for c in component.chars() {
                let is_word = c.is_ascii_alphanumeric() || c == '_';
                if is_word || crate::common::core::is_js_whitespace_char(c) || c == SEP {
                    reg_ex_pattern.push(c);
                } else if c == '*' {
                    reg_ex_pattern.push_str(&format!("[^{escaped_separator}]*"));
                } else if c == '?' {
                    reg_ex_pattern.push_str(&format!("[^{escaped_separator}]"));
                } else {
                    reg_ex_pattern.push_str(&js_identity_escape(c));
                }
            }

            first_component = false;
        }
    }

    reg_ex_pattern
}

/// JS `'\\' + c` inside a regex: an identity escape. The `regex` crate accepts
/// `\c` only for ASCII punctuation, so other characters go in a class.
fn js_identity_escape(c: char) -> String {
    if c.is_ascii_punctuation() {
        format!("\\{c}")
    } else {
        format!("[{c}]")
    }
}

// common/pathUtils.ts:543
pub fn is_directory_wildcard_pattern_present(file_spec: &str) -> bool {
    let path = normalize_path(file_spec);
    let path_components = get_path_components(&path);

    for component in &path_components {
        if component == "**" {
            return true;
        }
    }

    false
}

// common/pathUtils.ts:556
pub fn get_wildcard_root(root_path: &str, file_spec: &str) -> String {
    let mut absolute_path = normalize_path(&combine_paths(root_path, &[file_spec]));
    if !has_python_extension(&absolute_path) {
        absolute_path = ensure_trailing_directory_separator(&absolute_path);
    }

    let mut path_components = get_path_components(&absolute_path);
    let sep = get_path_separator(&absolute_path);

    if !path_components.is_empty() {
        path_components[0] = strip_trailing_directory_separator(&path_components[0]).to_string();
    }

    if path_components.len() == 1 && path_components[0].is_empty() {
        return sep.to_string();
    }

    let mut wildcard_root = String::new();
    let mut first_component = true;

    for component in &path_components {
        if component == "**" {
            break;
        } else {
            if component.contains(['*', '?']) {
                break;
            }

            if !first_component {
                wildcard_root.push_str(sep);
            }

            wildcard_root.push_str(component);
            first_component = false;
        }
    }

    wildcard_root
}

// common/pathUtils.ts:597
pub fn has_python_extension(path: &str) -> bool {
    path.ends_with(".py") || path.ends_with(".pyi")
}

// common/pathUtils.ts:601
pub fn get_regex_escaped_separator() -> &'static str {
    "/"
}

// common/pathUtils.ts:609
pub fn is_rooted_disk_path(path: &str) -> bool {
    get_root_length(path) > 0
}

// common/pathUtils.ts:616
pub fn is_disk_path_root(path: &str) -> bool {
    let root_length = get_root_length(path);
    root_length > 0 && root_length == path.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported from tests/pathUtils.test.ts (the POSIX cases).

    #[test]
    fn get_path_components1() {
        assert_eq!(get_path_components(""), vec![""]);
    }

    #[test]
    fn get_path_components2() {
        assert_eq!(get_path_components("/users/"), vec!["/", "users"]);
    }

    #[test]
    fn get_path_components3() {
        assert_eq!(
            get_path_components("/users/hello.py"),
            vec!["/", "users", "hello.py"]
        );
    }

    #[test]
    fn get_path_components4() {
        assert_eq!(get_path_components("/users/hello/../"), vec!["/", "users"]);
    }

    #[test]
    fn get_path_components5() {
        assert_eq!(get_path_components("./hello.py"), vec!["", "hello.py"]);
    }

    #[test]
    fn get_path_components6() {
        assert_eq!(
            get_path_components("//server/share/dir/file.py"),
            vec!["//server/", "share", "dir", "file.py"]
        );
    }

    #[test]
    fn combine_path_components1() {
        let p = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(combine_path_components(&p(&[])), "");
        assert_eq!(
            combine_path_components(&p(&["/", "users", "hello.py"])),
            "/users/hello.py"
        );
        assert_eq!(combine_path_components(&p(&["", "hello.py"])), "hello.py");
    }

    #[test]
    fn combine_paths1() {
        assert_eq!(combine_paths("/user", &["1", "2", "3"]), "/user/1/2/3");
        assert_eq!(
            combine_paths("/foo", &["ngnamespace", "ngmain"]),
            "/foo/ngnamespace/ngmain"
        );
        assert_eq!(combine_paths("/user", &["/1", "2"]), "/1/2");
    }

    #[test]
    fn ensure_trailing_directory_separator1() {
        assert_eq!(ensure_trailing_directory_separator("hello"), "hello/");
        assert_eq!(ensure_trailing_directory_separator("hello/"), "hello/");
    }

    #[test]
    fn has_trailing_directory_separator1() {
        assert!(!has_trailing_directory_separator("hello"));
        assert!(has_trailing_directory_separator("hello/"));
        assert!(has_trailing_directory_separator("hello\\"));
    }

    #[test]
    fn strip_trailing_directory_separator1() {
        assert_eq!(strip_trailing_directory_separator("hello/"), "hello");
        assert_eq!(strip_trailing_directory_separator("hello"), "hello");
    }

    #[test]
    fn get_file_extension1() {
        assert_eq!(get_file_extension("blah.blah/hello.JsOn", false), ".JsOn");
        assert_eq!(
            get_file_extension("blah.blah/hello.cpython-32m.so", true),
            ".cpython-32m.so"
        );
    }

    #[test]
    fn get_file_name1() {
        assert_eq!(get_file_name("blah.blah/HeLLo.JsOn"), "HeLLo.JsOn");
    }

    #[test]
    fn strip_file_extension1() {
        assert_eq!(
            strip_file_extension("blah.blah/HeLLo.JsOn", false),
            "blah.blah/HeLLo"
        );
        assert_eq!(
            strip_file_extension("blah.blah/hello.cpython-32m.so", true),
            "blah.blah/hello"
        );
    }

    #[test]
    fn get_wildcard_regex_pattern1() {
        let pattern = get_wildcard_regex_pattern("/users/me", "./blah/");
        let regex = Regex::new(&pattern).unwrap();
        assert!(regex.is_match("/users/me/blah/d"));
        assert!(!regex.is_match("/users/me/blad/d"));
    }

    #[test]
    fn get_wildcard_regex_pattern2() {
        let pattern = get_wildcard_regex_pattern("/users/me", "./**/*.py?");
        let regex = Regex::new(&pattern).unwrap();
        assert!(regex.is_match("/users/me/.blah/foo.pyd"));
        assert!(!regex.is_match("/users/me/.blah/foo.py"));
    }

    #[test]
    fn get_wildcard_root1() {
        assert_eq!(get_wildcard_root("/users/me", "./blah/"), "/users/me/blah");
        assert_eq!(get_wildcard_root("/users/me", "./**/*.py?/"), "/users/me");
    }

    #[test]
    fn get_wildcard_root2() {
        assert_eq!(get_wildcard_root("/", "."), "/");
    }

    #[test]
    fn is_rooted_disk_path1() {
        assert!(is_rooted_disk_path("C:/a/b"));
        assert!(is_rooted_disk_path("/"));
        assert!(!is_rooted_disk_path("a/b"));
    }

    #[test]
    fn is_disk_path_root1() {
        assert!(is_disk_path_root("/"));
        assert!(is_disk_path_root("c:/"));
        assert!(!is_disk_path_root("c:/a"));
    }

    #[test]
    fn get_relative_path1() {
        assert_eq!(get_relative_path("/a/b/c", "/a").as_deref(), Some("./b/c"));
        assert_eq!(get_relative_path("/x/b/c", "/a"), None);
    }

    #[test]
    fn contains_path1() {
        assert!(contains_path("/a/b", "/a/b/c", false));
        assert!(!contains_path("/a/b", "/a/c", false));
    }

    #[test]
    fn directory_path() {
        assert_eq!(get_directory_path("/a/b/c.py"), "/a/b");
        assert_eq!(get_directory_path("/a"), "/");
        assert_eq!(get_directory_path("abc"), "");
    }
}
