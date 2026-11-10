//! Port of `packages/pyright-internal/src/common/extraPathGlob.ts`.
//!
//! The watch-target helpers serve `--watch` and the language server and are
//! not ported.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;

use jsrt::{JsMap, JsSet, JsStr, NormalizationForm};
use regex::{Regex, RegexBuilder};

use crate::common::case_sensitivity_detector::CaseSensitivityDetector;
use crate::common::console::ConsoleInterface;
use crate::common::file_system::FileSystem;
use crate::common::path_utils::normalize_slashes;
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::{
    contains_wildcard_character, get_file_system_entries_with_symlinked_directories,
    get_wildcard_root, get_wildcard_segment_regex_fragment, is_directory, try_realpath,
};

// common/extraPathGlob.ts:39
pub fn contains_wildcard(entry: &str) -> bool {
    contains_wildcard_character(entry)
}

struct FixedCaseDetector(bool);

impl CaseSensitivityDetector for FixedCaseDetector {
    fn is_case_sensitive(&self, _uri: &str) -> bool {
        self.0
    }
}

// common/extraPathGlob.ts:108
pub fn expand_extra_paths(
    fs: &dyn FileSystem,
    base_uri: Uri,
    entries: &[String],
    console: Option<&dyn ConsoleInterface>,
) -> Vec<Uri> {
    let cleaned: Vec<&String> = entries
        .iter()
        .filter(|entry| entry.js_trim().js_len() > 0)
        .collect();

    let case_detector = FixedCaseDetector(base_uri.is_case_sensitive());

    // Phase 1: resolve every literal entry and claim its normalized key.
    let mut claimed_by_explicit: JsSet<String> = JsSet::default();
    let literal_by_index: Vec<Option<Uri>> = cleaned
        .iter()
        .map(|entry| {
            if contains_wildcard(entry) {
                return None;
            }
            let uri = if maybe_uri(entry) {
                Uri::parse(Some(entry), &case_detector)
            } else {
                base_uri.resolve_paths(&[entry])
            };
            claimed_by_explicit.insert(normalized_key(uri));
            Some(uri)
        })
        .collect();

    // Phase 2: emit entries in order.
    let mut result: Vec<Uri> = Vec::new();
    let mut emitted: JsSet<String> = JsSet::default();

    for (index, entry) in cleaned.iter().enumerate() {
        if let Some(literal) = literal_by_index[index] {
            let key = normalized_key(literal);
            if !emitted.contains(&key) {
                emitted.insert(key);
                result.push(literal);
            }
            continue;
        }

        if maybe_uri(entry) {
            if let Some(console) = console {
                console.info(&format!(
                    "Skipping glob expansion for non-path extra path \"{entry}\"."
                ));
            }
            continue;
        }

        for m in expand_glob_entry(fs, base_uri, entry, console) {
            let key = normalized_key(m);
            if claimed_by_explicit.contains(&key) || emitted.contains(&key) {
                continue;
            }
            emitted.insert(key);
            result.push(m);
        }
    }

    result
}

/// `Uri.maybeUri`: `/^[a-zA-Z][a-zA-Z0-9+.-]*:\/?\/?/` and not `/^[a-zA-Z]:\\?/`.
fn maybe_uri(value: &str) -> bool {
    let b = value.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphabetic() {
        return false;
    }
    let mut i = 1;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'+' | b'.' | b'-')) {
        i += 1;
    }
    i < b.len() && b[i] == b':' && !(b.len() >= 2 && b[1] == b':')
}

// common/extraPathGlob.ts:177
const LARGE_GLOB_SCAN_THRESHOLD: u32 = 10000;

// common/extraPathGlob.ts:184
const LARGE_GLOB_SCAN_HARD_LIMIT: u32 = 100000;

struct GlobWalk<'a> {
    fs: &'a dyn FileSystem,
    entry: &'a str,
    console: Option<&'a dyn ConsoleInterface>,
    tail: Vec<String>,
    case_sensitive: bool,
    matches: RefCell<JsMap<String, Uri>>,
    active_path: RefCell<JsSet<String>>,
    visited: RefCell<JsSet<String>>,
    directories_scanned: Cell<u32>,
    warned_large_scan: Cell<bool>,
    aborted_large_scan: Cell<bool>,
}

impl GlobWalk<'_> {
    fn visit(&self, dir_uri: Uri, tail_index: usize) {
        if self.aborted_large_scan.get() {
            return;
        }
        if tail_index == self.tail.len() {
            self.matches
                .borrow_mut()
                .insert(normalized_key(dir_uri), dir_uri);
            return;
        }

        let component = &self.tail[tail_index];
        if component == "**" {
            self.visit(dir_uri, tail_index + 1);
            for sub_dir in
                get_file_system_entries_with_symlinked_directories(self.fs, dir_uri).directories
            {
                self.descend(sub_dir, tail_index);
            }
        } else if contains_wildcard(component) {
            let segment_regex = segment_to_regex(component, self.case_sensitive);
            for sub_dir in
                get_file_system_entries_with_symlinked_directories(self.fs, dir_uri).directories
            {
                if segment_regex.is_match(sub_dir.file_name()) {
                    self.descend(sub_dir, tail_index + 1);
                }
            }
        } else {
            let next = dir_uri.combine_paths(&[component]);
            if is_directory(self.fs, next) {
                self.descend(next, tail_index + 1);
            }
        }
    }

    fn descend(&self, dir_uri: Uri, tail_index: usize) {
        if self.aborted_large_scan.get() {
            return;
        }
        let Some(real_path) = try_realpath(self.fs, dir_uri) else {
            return;
        };
        if self.active_path.borrow().contains(real_path.key()) {
            if let Some(console) = self.console {
                console.info(&format!(
                    "Skipping recursive symlink \"{}\".",
                    dir_uri.to_user_visible_string()
                ));
            }
            return;
        }
        let memo_key = format!("{}:{tail_index}", normalized_key(dir_uri));
        if self.visited.borrow().contains(&memo_key) {
            return;
        }
        self.visited.borrow_mut().insert(memo_key);
        self.directories_scanned
            .set(self.directories_scanned.get() + 1);
        let entry = self.entry;
        if !self.warned_large_scan.get()
            && self.directories_scanned.get() > LARGE_GLOB_SCAN_THRESHOLD
        {
            self.warned_large_scan.set(true);
            if let Some(console) = self.console {
                console.warn(&format!(
                    "Expanding the \"extraPaths\" glob \"{entry}\" has scanned more than {LARGE_GLOB_SCAN_THRESHOLD} \
                     directories, which can slow analysis. Consider narrowing the pattern (for example, avoid a bare \"**\")."
                ));
            }
        }
        if self.directories_scanned.get() > LARGE_GLOB_SCAN_HARD_LIMIT {
            self.aborted_large_scan.set(true);
            if let Some(console) = self.console {
                console.warn(&format!(
                    "Expanding the \"extraPaths\" glob \"{entry}\" exceeded the hard limit of {LARGE_GLOB_SCAN_HARD_LIMIT} \
                     directories and was aborted to protect analysis performance. Narrow the pattern (for example, \
                     avoid a bare \"**\" over a large or symlinked tree)."
                ));
            }
            return;
        }
        self.active_path
            .borrow_mut()
            .insert(real_path.key().to_string());
        self.visit(dir_uri, tail_index);
        self.active_path.borrow_mut().shift_remove(real_path.key());
    }
}

// common/extraPathGlob.ts:187
fn expand_glob_entry(
    fs: &dyn FileSystem,
    base_uri: Uri,
    entry: &str,
    console: Option<&dyn ConsoleInterface>,
) -> Vec<Uri> {
    let absolute = base_uri.resolve_paths(&[entry]);

    if !absolute.scheme().is_empty() && absolute.scheme() != "file" {
        if let Some(console) = console {
            console.info(&format!(
                "Skipping glob expansion for non-file extra path \"{entry}\"."
            ));
        }
        return Vec::new();
    }

    let wildcard_root = get_wildcard_root(base_uri, entry);
    let absolute_components = absolute.get_path_components();
    let root_components = wildcard_root.get_path_components();
    let raw_tail = &absolute_components[root_components.len().min(absolute_components.len())..];
    if raw_tail.is_empty() {
        return Vec::new();
    }

    // Collapse runs of consecutive `**` into a single `**`.
    let mut tail: Vec<String> = Vec::new();
    for component in raw_tail {
        if component == "**" && tail.last().is_some_and(|c| c == "**") {
            continue;
        }
        tail.push(component.clone());
    }

    let walk = GlobWalk {
        fs,
        entry,
        console,
        tail,
        case_sensitive: base_uri.is_case_sensitive(),
        matches: RefCell::new(JsMap::default()),
        active_path: RefCell::new(JsSet::default()),
        visited: RefCell::new(JsSet::default()),
        directories_scanned: Cell::new(0),
        warned_large_scan: Cell::new(false),
        aborted_large_scan: Cell::new(false),
    };
    walk.descend(wildcard_root, 0);

    let mut matches: Vec<Uri> = walk.matches.into_inner().into_values().collect();
    matches.sort_by(|a, b| {
        let key_a = sort_key(*a);
        let key_b = sort_key(*b);
        match key_a.js_cmp(&key_b) {
            Ordering::Equal => {
                let raw_a = normalize_slashes(a.get_file_path());
                let raw_b = normalize_slashes(b.get_file_path());
                raw_a.js_cmp(&raw_b)
            }
            other => other,
        }
    });
    matches
}

// common/extraPathGlob.ts:346
fn segment_to_regex(segment: &str, case_sensitive: bool) -> Regex {
    let pattern = get_wildcard_segment_regex_fragment(segment);
    RegexBuilder::new(&format!("^{pattern}$"))
        .case_insensitive(!case_sensitive)
        .build()
        .unwrap()
}

// common/extraPathGlob.ts:354
fn normalized_key(uri: Uri) -> String {
    let text = uri.to_uri_string();
    if text.js_len() > 1 && text.ends_with('/') {
        return text[..text.len() - 1].to_string();
    }
    text
}

// common/extraPathGlob.ts:372
fn sort_key(uri: Uri) -> String {
    normalize_slashes(uri.get_file_path()).js_normalize(NormalizationForm::Nfc)
}
