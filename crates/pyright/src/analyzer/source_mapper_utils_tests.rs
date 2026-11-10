//! Port of the `BuildImportTree` tests in `packages/pyright-internal/src/tests/sourceMapperUtils.test.ts`.
//!
//! The other tests there drive a `SourceMapper` through the language server's test harness.

use crate::analyzer::source_mapper_utils::build_import_tree as build_import_tree_impl;
use crate::common::case_sensitivity_detector::CaseSensitiveDetector;
use crate::common::uri::uri::Uri;

fn file(path: &str) -> Uri {
    Uri::file(path, &CaseSensitiveDetector, false)
}

fn build_import_tree(
    source_file: &str,
    target_file: &str,
    import_resolver: &dyn Fn(&str) -> Vec<String>,
) -> Vec<String> {
    build_import_tree_impl(
        &file(source_file),
        &file(target_file),
        &mut |from| {
            let resolved = import_resolver(&from.get_file_path()[1..]);
            resolved.iter().map(|f| file(f)).collect()
        },
        (),
    )
    .iter()
    .map(|u| u.get_file_path()[1..].to_string())
    .collect()
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[test]
fn simple() {
    let results = build_import_tree("A", "C", &|f| match f {
        "C" => strings(&["B"]),
        "B" => strings(&["A"]),
        _ => vec![],
    });
    assert_eq!(results, strings(&["C", "B"]));
}

#[test]
fn recursion() {
    let results = build_import_tree("A", "E", &|f| match f {
        "E" => strings(&["D"]),
        "D" => strings(&["C", "B"]),
        "C" => strings(&["D"]),
        "B" => strings(&["A"]),
        _ => vec![],
    });
    assert_eq!(results, strings(&["E", "D", "B"]));
}

#[test]
fn multiple_paths() {
    let results = build_import_tree("A", "G", &|f| match f {
        "G" => strings(&["F", "H", "I"]),
        "F" => strings(&["D", "E"]),
        "D" => strings(&["C", "B"]),
        "C" => strings(&["E"]),
        "B" => strings(&["A"]),
        _ => vec![],
    });
    assert_eq!(results, strings(&["G", "F", "D", "B"]));
}

#[test]
fn no_paths() {
    let results = build_import_tree("A", "G", &|f| match f {
        "G" => strings(&["F", "H", "I"]),
        "F" => strings(&["D", "E"]),
        "D" => strings(&["C", "B"]),
        "C" => strings(&["E"]),
        _ => vec![],
    });
    assert_eq!(results, strings(&["G"]));
}

#[test]
fn too_deep() {
    let results = build_import_tree("Z", "A", &|f| {
        let start = f.as_bytes()[0];
        let end = b'Y';
        (start..end).map(|c| (c as char).to_string()).collect()
    });
    assert_eq!(results, strings(&["A"]));
}

#[test]
#[ignore = "CancellationToken is () on the check path: nothing cancels the CLI check"]
fn canceled() {}
