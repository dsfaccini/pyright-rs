//! Port of `packages/pyright-internal/src/tests/pathUtils.test.ts`.
//!
//! POSIX host: `path.sep` is `/`, so `fixSeparators` and `normalizeSlashes`
//! on a path without backslashes return it unchanged.
//!
//! These TS tests are already ported as unit tests in
//! `crates/pyright/src/common/path_utils.rs` (`mod tests`), with every
//! assertion, so they are not repeated here:
//! - tests/pathUtils.test.ts:37 `getPathComponents1` → `get_path_components1`
//! - tests/pathUtils.test.ts:43 `getPathComponents2` → `get_path_components2`
//! - tests/pathUtils.test.ts:50 `getPathComponents3` → `get_path_components3`
//! - tests/pathUtils.test.ts:58 `getPathComponents4` → `get_path_components4`
//! - tests/pathUtils.test.ts:65 `getPathComponents5` → `get_path_components5`
//! - tests/pathUtils.test.ts:72 `getPathComponents6` → `get_path_components6`
//! - tests/pathUtils.test.ts:89 `combinePaths1` → `combine_paths1`
//! - tests/pathUtils.test.ts:104 `ensureTrailingDirectorySeparator1` → `ensure_trailing_directory_separator1`
//! - tests/pathUtils.test.ts:109 `hasTrailingDirectorySeparator1` → `has_trailing_directory_separator1`
//! - tests/pathUtils.test.ts:115 `stripTrailingDirectorySeparator1` → `strip_trailing_directory_separator1`
//! - tests/pathUtils.test.ts:120 `getFileExtension1` → `get_file_extension1`
//! - tests/pathUtils.test.ts:132 `getFileName1` → `get_file_name1`
//! - tests/pathUtils.test.ts:142 `stripFileExtension1` → `strip_file_extension1`
//! - tests/pathUtils.test.ts:161 `getWildcardRegexPattern1` → `get_wildcard_regex_pattern1`
//! - tests/pathUtils.test.ts:168 `getWildcardRegexPattern2` → `get_wildcard_regex_pattern2`
//! - tests/pathUtils.test.ts:216 `getWildcardRoot1` → `get_wildcard_root1`
//! - tests/pathUtils.test.ts:221 `getWildcardRoot2` → `get_wildcard_root1`
//! - tests/pathUtils.test.ts:226 `getWildcardRoot with root` → `get_wildcard_root2`
//! - tests/pathUtils.test.ts:244 `combinePathComponentsEmpty` → `combine_path_components1`
//! - tests/pathUtils.test.ts:328 `isRootedDiskPath1` → `is_rooted_disk_path1`
//! - tests/pathUtils.test.ts:332 `isRootedDiskPath2` → `is_rooted_disk_path1`
//! - tests/pathUtils.test.ts:336 `isRootedDiskPath3` → `is_rooted_disk_path1`
//! - tests/pathUtils.test.ts:340 `isDiskPathRoot1` → `is_rooted_disk_path1`

use pyright::common::path_utils;
use regex::Regex;

// tests/pathUtils.test.ts:81
#[test]
fn get_path_components7() {
    let components = path_utils::get_path_components("ab:cdef/test");
    assert_eq!(components.len(), 3);
    assert_eq!(components[0], "");
    assert_eq!(components[1], "ab:cdef");
    assert_eq!(components[2], "test");
}

// tests/pathUtils.test.ts:94
#[test]
fn combine_paths2() {
    let p = path_utils::combine_paths("/foo", &["ab:c"]);
    assert_eq!(p, path_utils::normalize_slashes("/foo/ab:c"));
}

// tests/pathUtils.test.ts:99
#[test]
fn combine_paths3() {
    let p = path_utils::combine_paths("untitled:foo", &["ab:c"]);
    assert_eq!(p, path_utils::normalize_slashes("untitled:foo/ab:c"));
}

// tests/pathUtils.test.ts:125
#[test]
fn get_file_extension2() {
    let ext1 = path_utils::get_file_extension("blah.blah/hello.cpython-32m.so", true);
    assert_eq!(ext1, ".cpython-32m.so");
    let ext2 = path_utils::get_file_extension("blah.blah/hello.cpython-32m.so", false);
    assert_eq!(ext2, ".so");
}

// tests/pathUtils.test.ts:137
#[test]
fn get_file_name2() {
    let file_name1 = path_utils::get_file_name("blah.blah/hello.cpython-32m.so");
    assert_eq!(file_name1, "hello.cpython-32m.so");
}

// tests/pathUtils.test.ts:147
#[test]
fn strip_file_extension2() {
    let path1 = path_utils::strip_file_extension("blah.blah/hello.cpython-32m.so", true);
    assert_eq!(path1, "blah.blah/hello");
    let path2 = path_utils::strip_file_extension("blah.blah/hello.cpython-32m.so", false);
    assert_eq!(path2, "blah.blah/hello.cpython-32m");
}

// tests/pathUtils.test.ts:175
#[test]
fn get_wildcard_regex_pattern3() {
    let pattern = path_utils::get_wildcard_regex_pattern("/users/me", "./**/.*.py");
    let regex = Regex::new(&pattern).unwrap();
    assert!(regex.is_match("/users/me/.blah/.foo.py"));
    assert!(!regex.is_match("/users/me/.blah/foo.py"));
}

// tests/pathUtils.test.ts:182
#[test]
fn get_wildcard_regex_pattern4() {
    let pattern = path_utils::get_wildcard_regex_pattern("//server/share/dir", ".");
    let regex = Regex::new(&pattern).unwrap();
    assert!(regex.is_match("//server/share/dir/foo.py"));
    assert!(!regex.is_match("//server/share/dix/foo.py"));
}

// tests/pathUtils.test.ts:189
#[test]
fn get_wildcard_regex_pattern5() {
    let pattern = path_utils::get_wildcard_regex_pattern("//server/share/dir++", ".");
    let regex = Regex::new(&pattern).unwrap();
    assert!(regex.is_match("//server/share/dir++/foo.py"));
    assert!(!regex.is_match("//server/share/dix++/foo.py"));
}

// tests/pathUtils.test.ts:196
#[test]
fn is_directory_wildcard_pattern_present1() {
    let is_present = path_utils::is_directory_wildcard_pattern_present("./**/*.py");
    assert!(is_present);
}

// tests/pathUtils.test.ts:201
#[test]
fn is_directory_wildcard_pattern_present2() {
    let is_present = path_utils::is_directory_wildcard_pattern_present("./**/a/*.py");
    assert!(is_present);
}

// tests/pathUtils.test.ts:206
#[test]
fn is_directory_wildcard_pattern_present3() {
    let is_present = path_utils::is_directory_wildcard_pattern_present("./**/@tests");
    assert!(is_present);
}

// tests/pathUtils.test.ts:211
#[test]
fn is_directory_wildcard_pattern_present4() {
    let is_present = path_utils::is_directory_wildcard_pattern_present("./**/test/test*");
    assert!(is_present);
}

// tests/pathUtils.test.ts:231
#[test]
fn get_wildcard_root_with_drive_letter() {
    let p = path_utils::get_wildcard_root("c:/", ".");
    assert_eq!(p, path_utils::normalize_slashes("c:"));
}

// tests/pathUtils.test.ts:236
#[test]
fn reduce_path_components_empty() {
    assert_eq!(path_utils::reduce_path_components(&[]).len(), 0);
}

// tests/pathUtils.test.ts:240
#[test]
fn reduce_path_components() {
    assert_eq!(
        path_utils::reduce_path_components(&path_utils::get_path_components("/a/b/../c/.")),
        vec!["/", "a", "c"]
    );
}

// tests/pathUtils.test.ts:248
#[test]
fn combine_path_components_absolute() {
    assert_eq!(
        path_utils::combine_path_components(&strings(&["/", "a", "b"])),
        path_utils::normalize_slashes("/a/b")
    );
}

// tests/pathUtils.test.ts:252
#[test]
fn combine_path_components() {
    assert_eq!(
        path_utils::combine_path_components(&strings(&["a", "b"])),
        path_utils::normalize_slashes("a/b")
    );
}

// tests/pathUtils.test.ts:256
#[test]
fn resolve_path1() {
    assert_eq!(
        path_utils::resolve_paths("/path", &["to", "file.ext"]),
        path_utils::normalize_slashes("/path/to/file.ext")
    );
}

// tests/pathUtils.test.ts:260
#[test]
fn resolve_path2() {
    assert_eq!(
        path_utils::resolve_paths("/path", &["to", "..", "from", "file.ext/"]),
        path_utils::normalize_slashes("/path/from/file.ext/")
    );
}

// tests/pathUtils.test.ts:264
#[test]
fn contains_path1() {
    assert!(path_utils::contains_path(
        "/a/b/c/",
        "/a/d/../b/c/./d",
        false
    ));
}

// tests/pathUtils.test.ts:268
#[test]
fn contains_path2() {
    assert!(path_utils::contains_path("/", "\\a", false));
}

// tests/pathUtils.test.ts:272
#[test]
fn contains_path3() {
    assert!(path_utils::contains_path("/a", "/A/B", true));
}

// tests/pathUtils.test.ts:276
#[test]
#[ignore = "getAnyExtensionFromPath(path, extensions, ignoreCase) overload is not ported (common/path_utils.rs)"]
fn change_any_extension2() {
    // assert.equal(getAnyExtensionFromPath('/path/to/file.ext', '.ts', true), '');
    panic!("get_any_extension_from_path has no extensions/ignore_case parameters");
}

// tests/pathUtils.test.ts:280
#[test]
#[ignore = "getAnyExtensionFromPath(path, extensions, ignoreCase) overload is not ported (common/path_utils.rs)"]
fn change_any_extension3() {
    // assert.equal(getAnyExtensionFromPath('/path/to/file.ext', ['.ext', '.ts'], true), '.ext');
    panic!("get_any_extension_from_path has no extensions/ignore_case parameters");
}

// tests/pathUtils.test.ts:284
#[test]
fn get_base_file_name1() {
    assert_eq!(
        path_utils::get_base_file_name("/path/to/file.ext"),
        "file.ext"
    );
}

// tests/pathUtils.test.ts:288
#[test]
fn get_base_file_name2() {
    assert_eq!(path_utils::get_base_file_name("/path/to/"), "to");
}

// tests/pathUtils.test.ts:292
#[test]
fn get_base_file_name3() {
    assert_eq!(path_utils::get_base_file_name("c:/"), "");
}

// tests/pathUtils.test.ts:296
#[test]
#[ignore = "getBaseFileName(path, extensions, ignoreCase) overload is not ported (common/path_utils.rs)"]
fn get_base_file_name4() {
    // assert.equal(getBaseFileName('/path/to/file.ext', ['.ext'], true), 'file');
    panic!("get_base_file_name has no extensions/ignore_case parameters");
}

// tests/pathUtils.test.ts:300
#[test]
fn get_root_length1() {
    assert_eq!(path_utils::get_root_length("a"), 0);
}

// tests/pathUtils.test.ts:304
#[test]
fn get_root_length2() {
    assert_eq!(path_utils::get_root_length("/"), 1);
}

// tests/pathUtils.test.ts:308
#[test]
fn get_root_length3() {
    assert_eq!(path_utils::get_root_length("c:"), 2);
}

// tests/pathUtils.test.ts:312
#[test]
fn get_root_length4() {
    assert_eq!(path_utils::get_root_length("c:d"), 0);
}

// tests/pathUtils.test.ts:316
#[test]
fn get_root_length5() {
    assert_eq!(path_utils::get_root_length("c:/"), 3);
}

// tests/pathUtils.test.ts:320
#[test]
fn get_root_length6() {
    assert_eq!(path_utils::get_root_length("//server"), 8);
}

// tests/pathUtils.test.ts:324
#[test]
fn get_root_length7() {
    assert_eq!(path_utils::get_root_length("//server/share"), 9);
}

// tests/pathUtils.test.ts:344
#[test]
fn is_disk_path_root2() {
    assert!(path_utils::is_rooted_disk_path(
        &path_utils::normalize_slashes("c:/")
    ));
}

// tests/pathUtils.test.ts:348
#[test]
fn is_disk_path_root3() {
    assert!(path_utils::is_rooted_disk_path(
        &path_utils::normalize_slashes("c:")
    ));
}

// tests/pathUtils.test.ts:352
#[test]
fn is_disk_path_root4() {
    assert!(!path_utils::is_rooted_disk_path(
        &path_utils::normalize_slashes("c:d")
    ));
}

// tests/pathUtils.test.ts:356
#[test]
fn get_relative_path() {
    assert_eq!(
        path_utils::get_relative_path(
            &path_utils::normalize_slashes("/a/b/c/d/e/f"),
            &path_utils::normalize_slashes("/a/b/c")
        )
        .as_deref(),
        Some(path_utils::normalize_slashes("./d/e/f").as_str())
    );
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}
