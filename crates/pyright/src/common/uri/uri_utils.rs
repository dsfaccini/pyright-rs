//! Port of `packages/pyright-internal/src/common/uri/uriUtils.ts`.

use std::cmp::Ordering;

use jsrt::JsStr;
use regex::{Regex, RegexBuilder};

use crate::common::file_system::{Dirent, ReadOnlyFileSystem, Stats};

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
    // common/uri/uriUtils.ts:46
    pub fn is_in_path(uri: Uri, paths: &[FileSpec]) -> bool {
        paths.iter().any(|p| uri.matches_regex(&p.reg_exp))
    }

    // common/uri/uriUtils.ts:50
    pub fn matches_include_file_regex(uri: Uri, is_file: bool) -> bool {
        if is_file {
            uri.matches_regex(include_file_regex())
        } else {
            true
        }
    }

    // common/uri/uriUtils.ts:54
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

// common/uri/uriUtils.ts:38
fn include_file_regex() -> &'static Regex {
    thread_local! {
        static RE: &'static Regex = Box::leak(Box::new(Regex::new(r"\.pyi?$").unwrap()));
    }
    RE.with(|r| *r)
}

// common/uri/uriUtils.ts:74
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

// common/uri/uriUtils.ts:65
#[derive(Clone, Debug, Default)]
pub struct FileSystemEntries {
    pub files: Vec<Uri>,
    pub directories: Vec<Uri>,
}

// common/uri/uriUtils.ts:70
#[derive(Clone, Debug, Default)]
pub struct FileSystemEntriesWithSymlinkedDirectories {
    pub files: Vec<Uri>,
    pub directories: Vec<Uri>,
    pub symlinked_directories: Vec<Uri>,
}

// common/uri/uriUtils.ts:111
pub fn get_file_size(fs: &dyn ReadOnlyFileSystem, uri: Uri) -> u64 {
    match try_stat(fs, uri) {
        Some(stat) if stat.is_file => stat.size,
        _ => 0,
    }
}

// common/uri/uriUtils.ts:119
pub fn file_exists(fs: &dyn ReadOnlyFileSystem, uri: Uri) -> bool {
    fs.stat_sync(uri).is_ok_and(|stat| stat.is_file)
}

// common/uri/uriUtils.ts:123
pub fn directory_exists(fs: &dyn ReadOnlyFileSystem, uri: Uri) -> bool {
    fs.stat_sync(uri).is_ok_and(|stat| stat.is_directory)
}

// common/uri/uriUtils.ts:127
pub fn is_directory(fs: &dyn ReadOnlyFileSystem, uri: Uri) -> bool {
    try_stat(fs, uri).is_some_and(|stat| stat.is_directory)
}

// common/uri/uriUtils.ts:134
pub fn is_usable_directory(fs: &dyn ReadOnlyFileSystem, uri: Uri) -> bool {
    fs.exists_sync(uri) && is_directory(fs, uri)
}

// common/uri/uriUtils.ts:142
pub fn get_usable_uri_path(fs: &dyn ReadOnlyFileSystem, uri: Option<Uri>) -> Option<String> {
    let uri = uri?;

    let uri_path = uri.get_file_path();
    if uri_path.is_empty() {
        return None;
    }

    if is_usable_directory(fs, uri) {
        Some(uri_path.to_string())
    } else {
        None
    }
}

// common/uri/uriUtils.ts:155
pub fn is_file(fs: &dyn ReadOnlyFileSystem, uri: Uri, treat_zip_directory_as_file: bool) -> bool {
    let stats = try_stat(fs, uri);
    if stats.is_some_and(|s| s.is_file) {
        return true;
    }

    if !treat_zip_directory_as_file {
        return false;
    }

    stats.is_some_and(|s| s.is_zip_directory)
}

// common/uri/uriUtils.ts:168
pub fn try_stat(fs: &dyn ReadOnlyFileSystem, uri: Uri) -> Option<Stats> {
    if fs.exists_sync(uri) {
        return fs.stat_sync(uri).ok();
    }
    None
}

// common/uri/uriUtils.ts:179
/// `realpathSync` never throws in any file system the CLI builds.
pub fn try_realpath(fs: &dyn ReadOnlyFileSystem, uri: Uri) -> Option<Uri> {
    Some(fs.realpath_sync(uri))
}

// common/uri/uriUtils.ts:187
pub fn get_file_system_entries(fs: &dyn ReadOnlyFileSystem, uri: Uri) -> FileSystemEntries {
    match fs.readdir_entries_sync(uri) {
        Ok(entries) => {
            let r = get_file_system_entries_with_symlinked_directories_from_dir_entries(
                entries, fs, uri,
            );
            FileSystemEntries {
                files: r.files,
                directories: r.directories,
            }
        }
        Err(_) => FileSystemEntries::default(),
    }
}

// common/uri/uriUtils.ts:200
pub fn get_file_system_entries_with_symlinked_directories(
    fs: &dyn ReadOnlyFileSystem,
    uri: Uri,
) -> FileSystemEntriesWithSymlinkedDirectories {
    match fs.readdir_entries_sync(uri) {
        Ok(entries) => {
            get_file_system_entries_with_symlinked_directories_from_dir_entries(entries, fs, uri)
        }
        Err(_) => FileSystemEntriesWithSymlinkedDirectories::default(),
    }
}

// common/uri/uriUtils.ts:212
pub fn get_file_system_entries_from_dir_entries(
    dir_entries: Vec<Dirent>,
    fs: &dyn ReadOnlyFileSystem,
    uri: Uri,
) -> FileSystemEntries {
    let r =
        get_file_system_entries_with_symlinked_directories_from_dir_entries(dir_entries, fs, uri);
    FileSystemEntries {
        files: r.files,
        directories: r.directories,
    }
}

// common/uri/uriUtils.ts:221
fn get_file_system_entries_with_symlinked_directories_from_dir_entries(
    dir_entries: Vec<Dirent>,
    fs: &dyn ReadOnlyFileSystem,
    uri: Uri,
) -> FileSystemEntriesWithSymlinkedDirectories {
    let mut entries = dir_entries;
    // The comparator is consistent, so any stable sort matches V8's.
    entries.sort_by(|a, b| match a.name.js_cmp(&b.name) {
        Ordering::Less => Ordering::Less,
        Ordering::Greater => Ordering::Greater,
        Ordering::Equal => Ordering::Equal,
    });
    let mut result = FileSystemEntriesWithSymlinkedDirectories::default();
    for entry in entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }

        let entry_uri = uri.combine_paths(&[&entry.name]);
        if entry.is_file {
            result.files.push(entry_uri);
        } else if entry.is_directory {
            result.directories.push(entry_uri);
        } else if entry.is_symbolic_link {
            let stat = try_stat(fs, entry_uri);
            if stat.is_some_and(|s| s.is_file) {
                result.files.push(entry_uri);
            } else if stat.is_some_and(|s| s.is_directory) {
                result.directories.push(entry_uri);
                result.symlinked_directories.push(entry_uri);
            }
        }
    }

    result
}

// common/uri/uriUtils.ts:269
pub fn contains_wildcard_character(text: &str) -> bool {
    text.contains(['*', '?'])
}

// common/uri/uriUtils.ts:278
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

// common/uri/uriUtils.ts:294
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

// common/uri/uriUtils.ts:325
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

// common/uri/uriUtils.ts:351
pub fn has_python_extension(uri: Uri) -> bool {
    uri.has_extension(".py") || uri.has_extension(".pyi")
}

// common/uri/uriUtils.ts:355
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
