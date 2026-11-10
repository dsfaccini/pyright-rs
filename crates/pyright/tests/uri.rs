//! Port of `packages/pyright-internal/src/tests/uri.test.ts`.
//!
//! POSIX host: `process.platform` is not `win32` and `normalizeSlashes` on a
//! path without backslashes returns it unchanged.
//!
//! `WebUri` is not ported (`crates/pyright/src/common/uri/uri.rs`): `Uri::parse`
//! panics on a non-file scheme such as `foo:`, `untitled:` or `memfs:`. Tests
//! that need one are ported in full and ignored.
//!
//! This TS test is already ported as a unit test in
//! `crates/pyright/src/common/uri/uri.rs` (`mod tests`), with every assertion,
//! so it is not repeated here:
//! - tests/uri.test.ts:1078 `constant uri test` → `empty_and_constant_identity`

use std::panic::catch_unwind;

use jsrt::{JsStr, node_path};
use pyright::common::case_sensitivity_detector::{CaseSensitiveDetector, CaseSensitivityDetector};
use pyright::common::console::NullConsole;
use pyright::common::file_system::ReadOnlyFileSystem;
use pyright::common::path_utils::{is_rooted_disk_path, normalize_slashes};
use pyright::common::real_file_system::{RealTempFile, create_from_real_file_system};
use pyright::common::uri::Uri;
use pyright::common::uri::uri_utils::{get_wildcard_regex_pattern, get_wildcard_root};
use regex::{Regex, RegexBuilder};

// tests/harness/testHost.ts:19
struct TestCaseSensitivityDetector {
    is_case_sensitive: bool,
}

impl CaseSensitivityDetector for TestCaseSensitivityDetector {
    // tests/harness/testHost.ts:28
    fn is_case_sensitive(&self, uri: &str) -> bool {
        if uri.starts_with("file") {
            return self.is_case_sensitive;
        }

        false
    }
}

// tests/uri.test.ts:30
const CASE_DETECTOR: TestCaseSensitivityDetector = TestCaseSensitivityDetector {
    is_case_sensitive: true,
};

/// `UriEx`'s detector for `isCaseSensitive === false`.
struct CaseInsensitiveDetector;

impl CaseSensitivityDetector for CaseInsensitiveDetector {
    fn is_case_sensitive(&self, _uri: &str) -> bool {
        false
    }
}

fn case_detector(is_case_sensitive: bool) -> &'static dyn CaseSensitivityDetector {
    if is_case_sensitive {
        &CaseSensitiveDetector
    } else {
        &CaseInsensitiveDetector
    }
}

/// `Uri.parse_uri(value, caseDetector)`.
fn parse_uri(value: &str) -> Uri {
    Uri::parse(Some(value), &CASE_DETECTOR)
}

// common/uri/uriUtils.ts:509
/// `UriEx.file(path)`.
fn uri_ex_file(path: &str) -> Uri {
    Uri::file(path, &CaseSensitiveDetector, false)
}

// common/uri/uriUtils.ts:509
/// `UriEx.file(path, isCaseSensitive)`.
fn uri_ex_file_case(path: &str, is_case_sensitive: bool) -> Uri {
    Uri::file(path, case_detector(is_case_sensitive), false)
}

// common/uri/uriUtils.ts:516
/// `UriEx.parse_uri(value)`.
fn uri_ex_parse(value: &str) -> Uri {
    Uri::parse(Some(value), &CaseSensitiveDetector)
}

// common/uri/uriUtils.ts:516
/// `UriEx.parse_uri(value, isCaseSensitive)`.
fn uri_ex_parse_case(value: &str, is_case_sensitive: bool) -> Uri {
    Uri::parse(Some(value), case_detector(is_case_sensitive))
}

fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

/// The jest cwd of the TS tests: `packages/pyright-internal` in the pyright
/// checkout at `~/pyright`.
fn pyright_internal_dir() -> String {
    let home = std::env::var("HOME").expect("HOME is set");
    format!("{home}/pyright/packages/pyright-internal")
}

/// Node `fs.readdirSync(path)`: libuv's scandir sorts the names with `strcmp`.
fn node_readdir_sync(path: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    names
}

/// Node `fs.realpathSync(path)`.
fn node_realpath_sync(path: &str) -> String {
    std::fs::canonicalize(path)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// Node `path.join(a, b)`.
fn node_path_join(a: &str, b: &str) -> String {
    node_path::normalize(&format!("{a}/{b}"))
}

// tests/uri.test.ts:32
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn parse() {
    assert!(catch_unwind(|| parse_uri("\\c:\\foo : bar")).is_err());
    assert!(catch_unwind(|| parse_uri("foo:////server/b/c")).is_err()); // No authority component
    parse_uri("foo:///a/b/c");
    parse_uri("foo:a/b/c");
    parse_uri("foo:/a/b/c");
    parse_uri("foo://server/share/dir/file.py");
    parse_uri("foo://server/share/dir/file.py?query#fragment");
    parse_uri("foo:///c:/users/me");
    parse_uri("foo:///c%3A%52users%52me");
    parse_uri("");
    Uri::parse(None, &CASE_DETECTOR);
}

// tests/uri.test.ts:46
#[test]
fn file() {
    // Like the TS test, this relies on the process cwd: `Uri.file` with
    // `checkRelative` resolves a relative path against it.
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let uri1 = Uri::file("a/b/c", &CASE_DETECTOR, true);
    assert!(uri1.get_file_path().js_len() > 6);
    assert!(
        uri1.get_file_path()
            .js_to_lower_case()
            .starts_with(&cwd.js_to_lower_case()),
        "{} does not start with {cwd}",
        uri1.get_file_path()
    );
    let uri2 = Uri::file("a/b/c", &CASE_DETECTOR, false);
    assert_eq!(uri2.get_file_path().js_len(), 6);
}

// tests/uri.test.ts:58
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn file_path() {
    // parse works with unix style file format
    assert_eq!(parse_uri("/folder1/folder2").scheme(), "file");

    // parse doesn't work with window style file format
    assert!(parse_uri("c:\\folder1\\folder2").scheme() != "file");

    // file works with both styles
    assert_eq!(
        Uri::file("/folder1/folder2", &CASE_DETECTOR, false).scheme(),
        "file"
    );
    assert_eq!(
        Uri::file("c:\\folder1\\folder2", &CASE_DETECTOR, false).scheme(),
        "file"
    );
}

// tests/uri.test.ts:70
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn key() {
    let key = parse_uri("foo:///a/b/c").key();
    let key2 = parse_uri("foo:///a/b/c").key();
    assert_eq!(key, key2);
    let key3 = parse_uri("foo:///a/b/d").key();
    assert_ne!(key, key3);
    let key4 = uri_ex_file("/a/b/c").key();
    assert_ne!(key, key4);
    let key5 = parse_uri("file:///a/b/c").key();
    assert_eq!(key4, key5);
    let key6 = uri_ex_file(&normalize_slashes("c:\\foo\\bar\\d.txt")).key();
    let key7 = parse_uri("file:///c%3A/foo/bar/d.txt").key();
    let key8 = parse_uri("file:///c:/foo/bar/d.txt").key();
    assert_eq!(key6, key7);
    assert_eq!(key6, key8);
    let key9 = uri_ex_parse_case("file:///c%3A/foo/bar/D.txt", true).key();
    let key10 = uri_ex_parse_case("file:///c:/foo/bar/d.txt", true).key();
    assert_ne!(key9, key10);
    let key11 = uri_ex_parse_case("file:///c%3A/foo/bar/D.txt", false).key();
    let key12 = uri_ex_parse_case("file:///c%3A/foo/bar/d.txt", false).key();
    assert_eq!(key11, key12);
}

// tests/uri.test.ts:93
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn filename() {
    let filename = parse_uri("foo:///a/b/c").file_name();
    assert_eq!(filename, "c");
    let filename2 = parse_uri("foo:///a/b/c/").file_name();
    assert_eq!(filename2, "c");
    let filename3 = parse_uri("foo:///a/b/c.py").file_name();
    assert_eq!(filename3, "c.py");
    let filename4 = parse_uri("foo:///a/b/c.py?query#fragment").file_name();
    assert_eq!(filename4, "c.py");
    let filename5 = uri_ex_file("/a/b/c").file_name();
    assert_eq!(filename5, "c");
    let filename6 = parse_uri("file:///a/b/c").file_name();
    assert_eq!(filename6, "c");
}

// tests/uri.test.ts:108
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn extname() {
    let extname = parse_uri("foo:///a/b/c").last_extension();
    assert_eq!(extname, "");
    let extname2 = parse_uri("foo:///a/b/c/").last_extension();
    assert_eq!(extname2, "");
    let extname3 = parse_uri("foo:///a/b/c.py").last_extension();
    assert_eq!(extname3, ".py");
    let extname4 = parse_uri("foo:///a/b/c.py?query#fragment").last_extension();
    assert_eq!(extname4, ".py");
    let extname5 = uri_ex_file("/a/b/c.py.foo").last_extension();
    assert_eq!(extname5, ".foo");
    let extname6 = parse_uri("file:///a/b/c.py.foo").last_extension();
    assert_eq!(extname6, ".foo");
}

// tests/uri.test.ts:123
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn fragment() {
    let fragment = parse_uri("foo:///a/b/c#bar").fragment();
    assert_eq!(fragment, "bar");
    let fragment2 = parse_uri("foo:///a/b/c#bar#baz").fragment();
    assert_eq!(fragment2, "bar#baz");
    let fragment3 = parse_uri("foo:///a/b/c?query#bar#baz").fragment();
    assert_eq!(fragment3, "bar#baz");
    let fragment4 = parse_uri("foo:///a/b/c?query").fragment();
    assert_eq!(fragment4, "");
    let fragment5 = parse_uri("foo:///a/b/c").with_fragment("bar").fragment();
    assert_eq!(fragment5, "bar");
    let fragment6 = parse_uri("foo:///a/b/c#bar").with_fragment("").fragment();
    assert_eq!(fragment6, "");
}

// tests/uri.test.ts:138
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn query() {
    let query = parse_uri("foo:///a/b/c?bar").query();
    assert_eq!(query, "bar");
    let query2 = parse_uri("foo:///a/b/c?bar?baz").query();
    assert_eq!(query2, "bar?baz");
    let query3 = parse_uri("foo:///a/b/c?bar?baz#fragment").query();
    assert_eq!(query3, "bar?baz");
    let query4 = parse_uri("foo:///a/b/c#fragment").query();
    assert_eq!(query4, "");
    let query5 = parse_uri("foo:///a/b/c").with_query("bar").query();
    assert_eq!(query5, "bar");
    let query6 = parse_uri("foo:///a/b/c?bar").with_query("").query();
    assert_eq!(query6, "");
}

// tests/uri.test.ts:153
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn contains_extension() {
    let uri1 = uri_ex_parse_case("foo:///a/b/c.py", true);
    assert!(uri1.contains_extension(".py"));
    assert!(!uri1.contains_extension(".PY"));
    assert!(!uri1.contains_extension(".pyi"));
    let uri2 = uri_ex_parse_case("foo:///a/b/c.pyi", true);
    assert!(uri2.contains_extension(".pyi"));
    assert!(!uri2.contains_extension(".PYI"));
    assert!(!uri2.contains_extension(".py"));
    let uri3 = uri_ex_parse_case("foo:///a/b/c.pyi.ipynb", false);
    assert!(uri3.contains_extension(".pyi"));
    assert!(uri3.contains_extension(".ipynb"));
    assert!(!uri3.contains_extension(".PYI"));
}

// tests/uri.test.ts:168
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn root() {
    let root1 = uri_ex_parse("foo://authority/a/b/c").root();
    assert_eq!(root1.to_uri_string(), "foo://authority/");
    let root = uri_ex_parse("file://server/b/c").root();
    assert_eq!(root.to_uri_string(), "file://server/");
    assert_eq!(root.get_root_path_length(), 9);
    let root2 = uri_ex_parse("foo:/").root();
    assert_eq!(root2.to_uri_string(), "foo:/");
    let root3 = uri_ex_parse("foo://a/b/c/").root();
    assert_eq!(root3.to_uri_string(), "foo://a/");
    assert!(root3.is_root());
    let root4 = uri_ex_parse("foo://a/b/c.py").root();
    assert_eq!(root4.to_uri_string(), "foo://a/");
    let root5 = uri_ex_parse("foo://a/b/c.py?query#fragment").root();
    assert_eq!(root5.to_uri_string(), "foo://a/");
    let root6 = uri_ex_file("/a/b/c.py.foo").root();
    assert_eq!(root6.to_uri_string(), "file:///");
    let root7 = uri_ex_parse("file:///a/b/c.py.foo").root();
    assert_eq!(root7.to_uri_string(), "file:///");
    assert_eq!(root7.get_root_path_length(), 1);
    let root8 = uri_ex_parse("untitled:Untitled-1").root();
    assert_eq!(root8.to_uri_string(), "untitled:");
    assert_eq!(root8.get_root_path_length(), 0);
    assert!(!root8.is_root());
    let root9 = uri_ex_parse("file://a/b/c/d.py").root();
    assert_eq!(root9.to_uri_string(), "file://a/");
    assert_eq!(root9.get_root_path_length(), 4);
    assert!(root9.is_root());
    let root10 = uri_ex_parse("file://c%3A/b/c/d.py").root();
    assert_eq!(root10.to_uri_string(), "file://c:/");
    assert_eq!(root10.get_root_path_length(), 5);
    assert!(root10.is_root());
}

// tests/uri.test.ts:202
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn untitled() {
    let untitled = uri_ex_parse_case("untitled:Untitled-1", true);
    assert_eq!(untitled.scheme(), "untitled");
    assert_eq!(untitled.file_name(), "Untitled-1");
    assert_eq!(untitled.to_uri_string(), "untitled:Untitled-1");
    let untitled2 = uri_ex_parse_case("untitled:Untitled-1", true);
    assert!(untitled.equals(Some(untitled2)));
    let untitled3 = uri_ex_parse_case("untitled:Untitled-2", true);
    assert!(!untitled.equals(Some(untitled3)));
    let untitled4 = uri_ex_parse_case("untitled:Untitled-1.foo.bar", false);
    assert_eq!(untitled4.scheme(), "untitled");
    assert_eq!(untitled4.file_name(), "Untitled-1.foo.bar");
    assert!(untitled4.contains_extension(".foo"));
    assert!(untitled4.contains_extension(".bar"));
}

// tests/uri.test.ts:218
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn empty() {
    let empty = parse_uri("");
    assert!(empty.is_empty());
    let empty2 = parse_uri("foo:///").is_empty();
    assert!(!empty2);
    let empty3 = Uri::empty();
    assert!(empty3.is_empty());
    let empty4 = Uri::parse(None, &CASE_DETECTOR);
    assert!(empty4.is_empty());
    assert!(empty4.equals(Some(empty3)));
    assert!(empty3.equals(Some(empty)));
    let combined = empty.combine_paths(&[&normalize_slashes("/d/e/f")]);
    assert_eq!(combined.get_file_path(), "");
}

// tests/uri.test.ts:233
/// The second TS test named `file`.
#[test]
fn file_2() {
    let file1 = uri_ex_file(&normalize_slashes("/a/b/c")).get_file_path();
    assert_eq!(file1, normalize_slashes("/a/b/c"));
    let file2 = uri_ex_file("file:///a/b/c").get_file_path();
    assert_eq!(file2, normalize_slashes("/a/b/c"));
    let resolved =
        uri_ex_file(&normalize_slashes("/a/b/c")).combine_paths(&[&normalize_slashes("/d/e/f")]);
    assert_eq!(resolved.get_file_path(), normalize_slashes("/d/e/f"));
}

// tests/uri.test.ts:242
#[test]
#[ignore = "Uri.is (a runtime type guard) has no Rust counterpart, and WebUri is not ported"]
fn is_uri() {
    // assert.equal(Uri.is('foo:///a/b/c'), false);
    // assert.equal(Uri.is('/a/b/c'), false);
    // assert.equal(Uri.is(undefined), false);
    // assert.equal(Uri.is(Uri.parse_uri('foo:///a/b/c', caseDetector)), true);
    // assert.equal(Uri.is(Uri.empty()), true);
    panic!("Uri.is is not ported");
}

// tests/uri.test.ts:255
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn matches_regex() {
    let include_files = Regex::new(r"\.pyi?$").unwrap();
    let uri = parse_uri("file:///a/b/c.pyi");
    assert!(uri.matches_regex(&include_files));
    let uri2 = parse_uri("file:///a/b/c.px");
    assert!(!uri2.matches_regex(&include_files));
    let uri3 = parse_uri("vscode-vfs:///a/b/c.pyi");
    assert!(uri3.matches_regex(&include_files));
    // JS `/^(c:\/foo\/bar)($|\/)/i`.
    let file_regex = RegexBuilder::new("^(c:/foo/bar)($|/)")
        .case_insensitive(true)
        .build()
        .unwrap();
    let uri4 = parse_uri("file:///C%3A/foo/bar");
    assert!(uri4.matches_regex(&file_regex));
    let uri5 = parse_uri("file:///c%3A/foo/bar");
    assert!(uri5.matches_regex(&file_regex));
    let uri6 = parse_uri("file:///c:/foo/bar");
    assert!(uri6.matches_regex(&file_regex));
    let uri7 = parse_uri("file:///c:/foo/bar/");
    assert!(uri7.matches_regex(&file_regex));
    let uri8 = parse_uri("file:///c:/foo/baz/");
    assert!(!uri8.matches_regex(&file_regex));
}

// tests/uri.test.ts:276
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn replace_extension() {
    let uri = parse_uri("file:///a/b/c.pyi");
    let uri2 = uri.replace_extension(".py");
    assert_eq!(uri2.to_uri_string(), "file:///a/b/c.py");
    let uri3 = parse_uri("file:///a/b/c");
    let uri4 = uri3.replace_extension(".py");
    assert_eq!(uri4.to_uri_string(), "file:///a/b/c.py");
    let uri5 = parse_uri("file:///a/b/c.foo.py");
    let uri6 = uri5.replace_extension(".pyi");
    assert_eq!(uri6.to_uri_string(), "file:///a/b/c.foo.pyi");
    let uri7 = parse_uri("memfs:/notebook.ipynb.py?query#fragment");
    let uri8 = uri7.replace_extension("");
    assert_eq!(uri8.to_uri_string(), "memfs:/notebook.ipynb");
    let uri9 = parse_uri("untitled:Untitled-1.ipynb.py?query#fragment");
    let uri10 = uri9.replace_extension("");
    assert_eq!(uri10.to_uri_string(), "untitled:Untitled-1.ipynb");
}

// tests/uri.test.ts:294
#[test]
fn add_extension() {
    let uri = parse_uri("file:///a/b/c.pyi?query#fragment");
    let uri2 = uri.add_extension(".py");
    assert_eq!(uri2.to_uri_string(), "file:///a/b/c.pyi.py");
    let uri3 = parse_uri("file:///a/b/c");
    let uri4 = uri3.add_extension(".py");
    assert_eq!(uri4.to_uri_string(), "file:///a/b/c.py");
}

// tests/uri.test.ts:303
#[test]
fn add_path() {
    let uri = parse_uri("file:///a/b/c.pyi?query#fragment");
    let uri2 = uri.add_path("d");
    assert_eq!(uri2.to_uri_string(), "file:///a/b/c.pyid");
}

// tests/uri.test.ts:309
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_directory() {
    let uri = parse_uri("file:///a/b/c.pyi?query#fragment");
    let uri2 = uri.get_directory();
    assert_eq!(uri2.to_uri_string(), "file:///a/b");
    let uri3 = uri2.get_directory();
    assert_eq!(uri3.to_uri_string(), "file:///a");
    let uri4 = parse_uri("file:///a/b/");
    let uri5 = uri4.get_directory();
    assert_eq!(uri5.to_uri_string(), "file:///a");
    let uri6 = uri4.get_directory();
    assert!(uri6.equals(Some(uri5)));
    let uri7 = uri5.get_directory();
    assert_eq!(uri7.to_uri_string(), "file:///");
    let uri8 = parse_uri("memfs:/a");
    let uri9 = uri8.get_directory();
    assert_eq!(uri9.to_uri_string(), "memfs:/");
    let uri10 = parse_uri("untitled:a");
    let uri11 = uri10.get_directory();
    assert_eq!(uri11.to_uri_string(), "untitled:");
}

// tests/uri.test.ts:330
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn init_and_pytyped() {
    let uri = parse_uri("file:///a/b/c?query#fragment");
    let uri2 = uri.pytyped_uri();
    assert_eq!(uri2.to_uri_string(), "file:///a/b/c/py.typed");
    let uri3 = uri.init_py_uri();
    assert_eq!(uri3.to_uri_string(), "file:///a/b/c/__init__.py");
    let uri4 = uri.init_pyi_uri();
    assert_eq!(uri4.to_uri_string(), "file:///a/b/c/__init__.pyi");
    let uri5 = uri.package_uri();
    assert_eq!(uri5.to_uri_string(), "file:///a/b/c.py");
    let uri6 = uri.package_stub_uri();
    assert_eq!(uri6.to_uri_string(), "file:///a/b/c.pyi");
    let uri7 = parse_uri("foo://microsoft.com/a/b/c.py");
    let uri8 = uri7.pytyped_uri();
    assert_eq!(
        uri8.to_uri_string(),
        "foo://microsoft.com/a/b/c.py/py.typed"
    );
    let uri9 = uri7.init_py_uri();
    assert_eq!(
        uri9.to_uri_string(),
        "foo://microsoft.com/a/b/c.py/__init__.py"
    );
    let uri10 = uri7.init_pyi_uri();
    assert_eq!(
        uri10.to_uri_string(),
        "foo://microsoft.com/a/b/c.py/__init__.pyi"
    );
    let uri11 = uri7.package_uri();
    assert_eq!(uri11.to_uri_string(), "foo://microsoft.com/a/b/c.py.py");
    let uri12 = uri7.package_stub_uri();
    assert_eq!(uri12.to_uri_string(), "foo://microsoft.com/a/b/c.py.pyi");
}

// tests/uri.test.ts:355
#[test]
fn is_child() {
    let parent = uri_ex_parse_case("file:///a/b/?query#fragment", true);
    let child = uri_ex_parse_case("file:///a/b/c.pyi?query#fragment", true);
    assert!(child.is_child(parent));
    let parent2 = uri_ex_parse_case("file:///a/b", true);
    let child2 = uri_ex_parse_case("file:///a/b/c.pyi", true);
    let child2_different_case = uri_ex_parse_case("file:///a/B/C.pyi", false);
    assert!(child2.is_child(parent2));
    assert!(child2_different_case.is_child(parent2));
    let parent3 = uri_ex_parse_case("file:///a/b/", true);
    let child3 = uri_ex_parse_case("file:///a/b/c.pyi", true);
    assert!(child3.is_child(parent3));
    let parent4 = uri_ex_parse_case("file:///a/b/", true);
    let not_child4 = uri_ex_parse_case("file:///a/bb/c.pyi", true);
    assert!(!not_child4.is_child(parent4));
    assert!(!not_child4.is_child(parent2));
    let not_child5 = uri_ex_parse_case("file:///a/b/", true);
    assert!(!not_child5.is_child(parent4));
}

// tests/uri.test.ts:375
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn equals() {
    let uri1 = uri_ex_parse_case("file:///a/b/c.pyi?query#fragment", true);
    let uri2 = uri_ex_file("/a/b/c.pyi");
    assert!(!uri1.equals(Some(uri2)));
    let uri3 = uri1.strip_extension().add_extension(".pyi");
    assert!(uri2.equals(Some(uri3)));
    let uri4 = uri_ex_parse_case("foo:///a/b/c", true);
    let uri5 = uri_ex_parse_case("foo:///a/b/c", true);
    let uri6 = uri_ex_parse_case("foo:///a/b/c/", true);
    assert!(uri4.equals(Some(uri5)));
    assert!(uri4.equals(Some(uri6)));
    let uri7 = uri_ex_parse_case("file://c%3A/b/c/d.py", true).root();
    let uri8 = uri_ex_parse_case("file://c:/", true);
    assert!(uri7.equals(Some(uri8)));
    let uri9 = uri_ex_parse_case("foo:///a/b/c?query", true);
    assert!(!uri9.equals(Some(uri4)));
    // Web uris are always case sensitive
    let uri10 = uri_ex_parse_case("foo:///a/b/c", false);
    let uri11 = uri_ex_parse_case("foo:///a/B/c", false);
    assert!(!uri10.equals(Some(uri11)));
    // Filre uris pay attention to the parameter.
    let uri12 = uri_ex_parse_case("file:///a/b/c", false);
    let uri13 = uri_ex_parse_case("file:///a/B/c", false);
    assert!(uri12.equals(Some(uri13)));
    let uri14 = uri_ex_parse_case("file:///a/b/c", true);
    let uri15 = uri_ex_parse_case("file:///a/B/c", true);
    assert!(!uri14.equals(Some(uri15)));
}

// tests/uri.test.ts:404
#[test]
fn starts_with() {
    let parent = parse_uri("file:///a/b/?query#fragment");
    let child = parse_uri("file:///a/b/c.pyi?query#fragment");
    assert!(child.starts_with(Some(parent)));
    let parent2 = parse_uri("file:///a/b");
    let child2 = parse_uri("file:///a/b/c.pyi");
    assert!(child2.starts_with(Some(parent2)));
    let parent3 = parse_uri("file:///a/b/");
    let child3 = parse_uri("file:///a/b/c.pyi");
    assert!(child3.starts_with(Some(parent3)));
    let parent4 = parse_uri("file:///a/b/");
    let not_child4 = parse_uri("file:///a/bb/c.pyi");
    assert!(!not_child4.starts_with(Some(parent4)));
    assert!(!not_child4.starts_with(Some(parent2)));
}

// tests/uri.test.ts:420
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn path_comparisons() {
    let uri = parse_uri("foo:///a/b/c.pyi?query#fragment");
    assert!(uri.path_ends_with("c.pyi"));
    assert!(uri.path_ends_with("b/c.pyi"));
    assert!(uri.path_ends_with("a/b/c.pyi"));
    assert!(!uri.path_ends_with("a/b/c.py"));
    assert!(!uri.path_ends_with("b/c.py"));
    assert!(uri.path_includes("c.pyi"));
    assert!(uri.path_includes("b/c"));
    assert!(uri.path_includes("a/b/c"));
    let uri2 = parse_uri("file:///C%3A/a/b/c.pyi?query#fragment");
    assert!(uri2.path_ends_with("c.pyi"));
    assert!(uri2.path_ends_with("b/c.pyi"));
    assert!(!uri2.path_starts_with("C:/a"));
    assert!(!uri2.path_starts_with("C:/a/b"));
    assert!(uri2.path_starts_with("c:/a"));
    assert!(uri2.path_starts_with("c:/a/b"));
}

// tests/uri.test.ts:439
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn combine_paths() {
    let uri1 = parse_uri("file:///a/b/c.pyi?query#fragment");
    let uri2 = uri1.combine_paths(&["d", "e"]);
    assert_eq!(uri2.to_uri_string(), "file:///a/b/c.pyi/d/e");
    let uri4 = uri1.combine_paths(&["d", "e", "f"]);
    assert_eq!(uri4.to_uri_string(), "file:///a/b/c.pyi/d/e/f");
    let uri5 = uri1.combine_paths(&["d", "..", "e"]);
    assert_eq!(uri5.to_uri_string(), "file:///a/b/c.pyi/e");
    let rooted_path = "/D";
    let rooted_result = "file:///D/e/f";
    let uri6 = uri1.combine_paths(&[rooted_path, "e", "f"]);
    assert_eq!(uri6.to_uri_string(), rooted_result);
    let uri7 = parse_uri("foo:");
    let uri8 = uri7.combine_paths(&["d", "e"]);
    assert_eq!(uri8.to_uri_string(), "foo:d/e");
    let uri9 = parse_uri("foo:/");
    let uri10 = uri9.combine_paths(&["d", "e"]);
    assert_eq!(uri10.to_uri_string(), "foo:/d/e");
    let uri11 = Uri::empty().combine_paths(&["d", "e"]);
    assert_eq!(uri11.to_uri_string(), "");
    let uri12 = uri1.combine_paths(&["d", "e", "f/"]);
    assert_eq!(uri12.to_uri_string(), "file:///a/b/c.pyi/d/e/f");
}

// tests/uri.test.ts:463
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn combine_paths_unsafe() {
    let uri1 = parse_uri("file:///a/b/c.pyi?query#fragment");
    let uri2 = uri1.combine_paths_unsafe(&["d", "e"]);
    assert_eq!(uri2.to_uri_string(), "file:///a/b/c.pyi/d/e");
    let uri4 = uri1.combine_paths_unsafe(&["d", "e", "f"]);
    assert_eq!(uri4.to_uri_string(), "file:///a/b/c.pyi/d/e/f");
    let uri5 = uri1.combine_paths_unsafe(&["d", "..", "e"]);
    assert_eq!(uri5.to_uri_string(), "file:///a/b/c.pyi/d/../e");
    let rooted_path = "/D";
    let rooted_result = "file:///D/e/f";
    let uri6 = uri1.combine_paths_unsafe(&[rooted_path, "e", "f"]);
    assert_eq!(uri6.to_uri_string(), rooted_result);
    let uri7 = parse_uri("foo:");
    let uri8 = uri7.combine_paths_unsafe(&["d", "e"]);
    assert_eq!(uri8.to_uri_string(), "foo:d/e");
    let uri9 = parse_uri("foo:/");
    let uri10 = uri9.combine_paths_unsafe(&["d", "e"]);
    assert_eq!(uri10.to_uri_string(), "foo:/d/e");
    let uri11 = Uri::empty().combine_paths_unsafe(&["d", "e"]);
    assert_eq!(uri11.to_uri_string(), "");
    let uri12 = uri1.combine_paths_unsafe(&["d", "e", "f/"]);
    assert_eq!(uri12.to_uri_string(), "file:///a/b/c.pyi/d/e/f/");
}

// tests/uri.test.ts:487
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn resolve_paths() {
    let uri1 = parse_uri("file:///a/b/c.pyi?query#fragment");
    let uri2 = uri1.resolve_paths(&["d", "e"]);
    assert_eq!(uri2.to_uri_string(), "file:///a/b/c.pyi/d/e");
    let uri3 = uri1.resolve_paths(&["d", "e/"]);
    assert_eq!(uri3.to_uri_string(), "file:///a/b/c.pyi/d/e");
    let uri4 = uri1.resolve_paths(&["d", "e", "f/"]);
    assert_eq!(uri4.to_uri_string(), "file:///a/b/c.pyi/d/e/f");
    let uri5 = uri1.resolve_paths(&["d", "..", "e"]);
    assert_eq!(uri5.to_uri_string(), "file:///a/b/c.pyi/e");
    let rooted_path = "/D";
    let rooted_result = "file:///D/e/f";
    let uri6 = uri1.resolve_paths(&[rooted_path, "e", "f"]);
    assert_eq!(uri6.to_uri_string(), rooted_result);
    let uri7 = parse_uri("foo:");
    let uri8 = uri7.resolve_paths(&["d", "e"]);
    assert_eq!(uri8.to_uri_string(), "foo:d/e");
    let uri9 = parse_uri("foo:/");
    let uri10 = uri9.resolve_paths(&["d", "e"]);
    assert_eq!(uri10.to_uri_string(), "foo:/d/e");
    let uri11 = Uri::empty().resolve_paths(&["d", "e"]);
    assert_eq!(uri11.to_uri_string(), "");
}

// tests/uri.test.ts:511
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn combine_paths_non_file() {
    let uri1 = parse_uri("baz://authority/a/b/c.pyi?query#fragment");
    let uri2 = uri1.combine_paths(&["d", "e"]);
    assert_eq!(uri2.to_uri_string(), "baz://authority/a/b/c.pyi/d/e");
    let uri4 = uri1.combine_paths(&["d", "e", "f"]);
    assert_eq!(uri4.to_uri_string(), "baz://authority/a/b/c.pyi/d/e/f");
}

// tests/uri.test.ts:519
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn resolve_paths_non_file() {
    let uri1 = parse_uri("baz://authority/a/b/c.pyi?query#fragment");
    let uri2 = uri1.resolve_paths(&["d", "e"]);
    assert_eq!(uri2.to_uri_string(), "baz://authority/a/b/c.pyi/d/e");
    let uri3 = uri1.resolve_paths(&["d", "e/"]);
    assert_eq!(uri3.to_uri_string(), "baz://authority/a/b/c.pyi/d/e");
    let uri4 = uri1.resolve_paths(&["d", "e", "f"]);
    assert_eq!(uri4.to_uri_string(), "baz://authority/a/b/c.pyi/d/e/f");
    let uri5 = uri1.resolve_paths(&["d", "..", "e"]);
    assert_eq!(uri5.to_uri_string(), "baz://authority/a/b/c.pyi/e");
}

// tests/uri.test.ts:531
#[test]
fn get_path_components1() {
    let components = parse_uri("").get_path_components();
    assert_eq!(components.len(), 0);
}

// tests/uri.test.ts:536
#[test]
fn get_path_components2() {
    let components = parse_uri("/users/").get_path_components();
    assert_eq!(components.len(), 2);
    assert_eq!(components[0], "/");
    assert_eq!(components[1], "users");
}

// tests/uri.test.ts:543
#[test]
fn get_path_components3() {
    let components = parse_uri("/users/hello.py").get_path_components();
    assert_eq!(components.len(), 3);
    assert_eq!(components[0], "/");
    assert_eq!(components[1], "users");
    assert_eq!(components[2], "hello.py");
}

// tests/uri.test.ts:551
#[test]
fn get_path_components4() {
    let components = parse_uri("/users/hello/../").get_path_components();
    assert_eq!(components.len(), 2);
    assert_eq!(components[0], "/");
    assert_eq!(components[1], "users");
}

// tests/uri.test.ts:558
#[test]
fn get_path_components5() {
    let components = parse_uri("./hello.py").get_path_components();
    assert_eq!(components.len(), 2);
    assert_eq!(components[0], "/");
    assert_eq!(components[1], "hello.py");
}

// tests/uri.test.ts:565
#[test]
fn get_path_components6() {
    let components = parse_uri("file://server/share/dir/file.py").get_path_components();
    assert_eq!(components.len(), 4);
    assert!(components[0].js_slice(2, None).contains("server"));
    assert_eq!(components[1], "share");
    assert_eq!(components[2], "dir");
    assert_eq!(components[3], "file.py");
}

// tests/uri.test.ts:574
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_relative_path_components1() {
    let components =
        parse_uri("foo:///users/").get_relative_path_components(parse_uri("foo:///users/"));
    assert_eq!(components.len(), 0);
}

// tests/uri.test.ts:581
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_relative_path_components2() {
    let components =
        parse_uri("foo:///users/").get_relative_path_components(parse_uri("foo:///users/bar"));
    assert_eq!(components.len(), 1);
    assert_eq!(components[0], "bar");
}

// tests/uri.test.ts:589
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_relative_path_components3() {
    let components =
        parse_uri("bar:///users/").get_relative_path_components(parse_uri("foo:///users/bar"));
    assert_eq!(components.len(), 1);
    assert_eq!(components[0], "bar");
}

// tests/uri.test.ts:597
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_relative_path_components4() {
    let components =
        parse_uri("foo:///users").get_relative_path_components(parse_uri("foo:///users/"));
    assert_eq!(components.len(), 0);
}

// tests/uri.test.ts:604
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_relative_path_components5() {
    let components = parse_uri("foo:///users/")
        .get_relative_path_components(parse_uri("foo:///users/bar/baz/../foo"));
    assert_eq!(components.len(), 2);
    assert_eq!(components[0], "bar");
    assert_eq!(components[1], "foo");
}

// tests/uri.test.ts:613
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_relative_path_components6() {
    let components =
        parse_uri("foo:///users/bar").get_relative_path_components(parse_uri("foo:///users/foo"));
    assert_eq!(components.len(), 2);
    assert_eq!(components[0], "..");
    assert_eq!(components[1], "foo");
}

// tests/uri.test.ts:622
#[test]
fn get_relative_path_components7() {
    let components = uri_ex_file_case("\\\\SERVER\\share\\users", false)
        .get_relative_path_components(uri_ex_file_case("\\\\server\\ShArE\\users\\bar", false));
    assert_eq!(components.len(), 1);
    assert_eq!(components[0], "bar");
}

// tests/uri.test.ts:630
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_file_extension1() {
    let ext = parse_uri("foo:///blah.blah/hello.JsOn").last_extension();
    assert_eq!(ext, ".JsOn");
}

// tests/uri.test.ts:635
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_file_name1() {
    let file_name = parse_uri("foo:///blah.blah/HeLLo.JsOn").file_name();
    assert_eq!(file_name, "HeLLo.JsOn");
}

// tests/uri.test.ts:640
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_file_name2() {
    let file_name1 = parse_uri("foo:///blah.blah/hello.cpython-32m.so").file_name();
    assert_eq!(file_name1, "hello.cpython-32m.so");
}

// tests/uri.test.ts:645
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn strip_file_extension1() {
    let path = parse_uri("foo:///blah.blah/HeLLo.JsOn")
        .strip_extension()
        .get_path();
    assert_eq!(path, "/blah.blah/HeLLo");
}

// tests/uri.test.ts:650
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn strip_file_extension2() {
    let path1 = parse_uri("foo:/blah.blah/hello.cpython-32m.so")
        .strip_all_extensions()
        .get_path();
    assert_eq!(path1, "/blah.blah/hello");
    let path2 = parse_uri("foo:/blah.blah/hello.cpython-32m.so")
        .strip_extension()
        .get_path();
    assert_eq!(path2, "/blah.blah/hello.cpython-32m");
}

// tests/uri.test.ts:657
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_wildcard_regex_pattern1() {
    let pattern = get_wildcard_regex_pattern(parse_uri("foo:///users/me"), "./blah/");
    let regex = Regex::new(&pattern).unwrap();
    assert!(regex.is_match("/users/me/blah/d"));
    assert!(!regex.is_match("/users/me/blad/d"));
}

// tests/uri.test.ts:664
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_wildcard_regex_pattern2() {
    let pattern = get_wildcard_regex_pattern(parse_uri("foo:///users/me"), "./**/*.py?");
    let regex = Regex::new(&pattern).unwrap();
    assert!(regex.is_match("/users/me/.blah/foo.pyd"));
    assert!(!regex.is_match("/users/me/.blah/foo.py")); // No char after
}

// tests/uri.test.ts:671
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_wildcard_regex_pattern3() {
    let pattern = get_wildcard_regex_pattern(parse_uri("foo:///users/me"), "./**/.*.py");
    let regex = Regex::new(&pattern).unwrap();
    assert!(regex.is_match("/users/me/.blah/.foo.py"));
    assert!(!regex.is_match("/users/me/.blah/foo.py"));
}

// tests/uri.test.ts:678
#[test]
fn get_wildcard_regex_pattern4() {
    let pattern = get_wildcard_regex_pattern(parse_uri("//server/share/dir"), ".");
    let regex = Regex::new(&pattern).unwrap();
    assert!(regex.is_match("//server/share/dir/foo.py"));
    assert!(!regex.is_match("//server/share/dix/foo.py"));
}

// tests/uri.test.ts:685
/// The second TS test named `getWildcardRegexPattern4`.
#[test]
fn get_wildcard_regex_pattern4_2() {
    let pattern = get_wildcard_regex_pattern(parse_uri("//server/share/dir++/.bar*/bid"), ".");
    let regex = Regex::new(&pattern).unwrap();
    assert!(regex.is_match("//server/share/dir++/.bar*/bidfoo.py"));
    assert!(!regex.is_match("//server/share/dix++/.bar*/bidfoo.py"));
}

// tests/uri.test.ts:692
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_wildcard_root1() {
    let p = get_wildcard_root(parse_uri("foo:/users/me"), "./blah/");
    assert_eq!(p.to_uri_string(), "foo:/users/me/blah");
}

// tests/uri.test.ts:697
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_wildcard_root2() {
    let p = get_wildcard_root(parse_uri("foo:/users/me"), "./**/*.py?/");
    assert_eq!(p.to_uri_string(), "foo:/users/me");
}

// tests/uri.test.ts:702
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn get_wildcard_root_with_root() {
    let p = get_wildcard_root(parse_uri("foo:/"), ".");
    assert_eq!(p.to_uri_string(), "foo:/");
}

// tests/uri.test.ts:707
#[test]
fn get_wildcard_root_with_drive_letter() {
    let p = get_wildcard_root(parse_uri("file:///c:/"), ".");
    assert_eq!(p.to_uri_string(), "file:///c%3A/");
}

// tests/uri.test.ts:712
fn resolve_paths_helper(uri: &str, paths: &[&str]) -> String {
    uri_ex_file(uri).resolve_paths(paths).to_uri_string()
}

// tests/uri.test.ts:718
#[test]
fn resolve_path1() {
    assert_eq!(
        resolve_paths_helper("/path", &["to", "file.ext"]),
        "file:///path/to/file.ext"
    );
}

// tests/uri.test.ts:722
#[test]
fn resolve_path2() {
    assert_eq!(
        resolve_paths_helper("/path", &["to", "..", "from", "file.ext/"]),
        "file:///path/from/file.ext"
    );
}

// tests/uri.test.ts:730
#[test]
#[ignore = "expandPathVariables is not ported (common/env_var_utils.rs is empty)"]
fn resolve_path3_escape() {
    // assert.equal(
    //     resolvePaths(expandPathVariables('~/path', Uri.empty(), []), 'to', '..', 'from', 'file.ext/'),
    //     `${getHomeDirUri().toString()}/path/from/file.ext`
    // );
    panic!("expand_path_variables is not ported");
}

// tests/uri.test.ts:737
#[test]
#[ignore = "expandPathVariables is not ported (common/env_var_utils.rs is empty)"]
fn resolve_path4_escape_in_middle() {
    // assert.equal(
    //     resolvePaths('/path', expandPathVariables('~/file.ext/', Uri.empty(), [])),
    //     `${getHomeDirUri().toString()}/file.ext`
    // );
    panic!("expand_path_variables is not ported");
}

// tests/uri.test.ts:748
#[test]
#[ignore = "expandPathVariables is not ported (common/env_var_utils.rs is empty), and WebUri is not ported"]
fn invalid_without_root() {
    // const path = combinePaths('Library', 'Mobile Documents', 'com~apple~CloudDocs', 'Development', 'mysuperproject');
    // assert.equal(resolvePaths(expandPathVariables(path, Uri.parse_uri('foo:///src', caseDetector), [])), path);
    panic!("expand_path_variables is not ported");
}

// tests/uri.test.ts:753
#[test]
#[ignore = "expandPathVariables is not ported (common/env_var_utils.rs is empty), and WebUri is not ported"]
fn invalid_with_root() {
    // const path = combinePaths('/', 'Library', 'com~apple~CloudDocs', 'Development', 'mysuperproject');
    // assert.equal(resolvePaths(expandPathVariables(path, Uri.parse_uri('foo:///src', caseDetector), [])), path);
    panic!("expand_path_variables is not ported");
}

// tests/uri.test.ts:758
fn contains_path(uri: &str, child: &str) -> bool {
    parse_uri(child).is_child(parse_uri(uri))
}

// tests/uri.test.ts:762
#[test]
fn contains_path1() {
    assert!(contains_path("/a/b/c/", "/a/d/../b/c/./d"));
}

// tests/uri.test.ts:766
#[test]
fn contains_path2() {
    assert!(contains_path("/", "\\a"));
}

// tests/uri.test.ts:770
#[test]
fn contains_path3() {
    assert!(contains_path("/a", "/a/B"));
}

// tests/uri.test.ts:774
fn get_any_extension_from_path(uri: &str) -> &'static str {
    parse_uri(uri).last_extension()
}

// tests/uri.test.ts:777
#[test]
fn get_any_extension1() {
    assert_eq!(get_any_extension_from_path("/path/to/file.ext"), ".ext");
}

// tests/uri.test.ts:781
fn get_base_file_name(uri: &str) -> &'static str {
    parse_uri(uri).file_name()
}

// tests/uri.test.ts:785
#[test]
fn get_base_file_name1() {
    assert_eq!(get_base_file_name("/path/to/file.ext"), "file.ext");
}

// tests/uri.test.ts:789
#[test]
fn get_base_file_name2() {
    assert_eq!(get_base_file_name("/path/to/"), "to");
}

// tests/uri.test.ts:793
#[test]
#[ignore = "WebUri is not ported: Uri::parse('c:/') has scheme `c` and panics (common/uri/uri.rs)"]
fn get_base_file_name3() {
    assert_eq!(get_base_file_name("c:/"), "");
}

// tests/uri.test.ts:797
fn get_uri_root_length(uri: &str) -> usize {
    uri_ex_file(uri).get_root_path_length()
}

// tests/uri.test.ts:801
#[test]
fn get_root_length1() {
    assert_eq!(get_uri_root_length("a"), 1);
}

// tests/uri.test.ts:805
#[test]
fn get_root_length2() {
    assert_eq!(get_uri_root_length("/"), 1);
}

// tests/uri.test.ts:809
#[test]
fn get_root_length3() {
    assert_eq!(get_uri_root_length("c:"), 3);
}

// tests/uri.test.ts:813
#[test]
fn get_root_length4() {
    assert_eq!(get_uri_root_length("c:d"), 0);
}

// tests/uri.test.ts:817
#[test]
fn get_root_length5() {
    assert_eq!(get_uri_root_length("c:/"), 3);
}

// tests/uri.test.ts:821
#[test]
fn get_root_length6() {
    assert_eq!(get_uri_root_length("//server"), 9);
}

// tests/uri.test.ts:825
#[test]
fn get_root_length7() {
    assert_eq!(get_uri_root_length("//server/share"), 9);
}

// tests/uri.test.ts:829
#[test]
fn get_root_length8() {
    assert_eq!(get_uri_root_length("scheme:/no/authority"), 1);
}

// tests/uri.test.ts:833
#[test]
fn get_root_length9() {
    assert_eq!(get_uri_root_length("scheme://with/authority"), 1);
}

// tests/uri.test.ts:837
fn is_rooted_disk_uri(uri: &str) -> bool {
    is_rooted_disk_path(uri_ex_file(uri).get_file_path())
}

// tests/uri.test.ts:841
#[test]
fn is_rooted_disk_path1() {
    assert!(is_rooted_disk_uri("C:/a/b"));
}

// tests/uri.test.ts:845
#[test]
fn is_rooted_disk_path2() {
    assert!(is_rooted_disk_uri("/"));
}

// tests/uri.test.ts:849
#[test]
fn is_rooted_disk_path3() {
    assert!(is_rooted_disk_uri("a/b"));
}

// tests/uri.test.ts:853
#[test]
fn is_disk_path_root1() {
    assert!(is_rooted_disk_uri("/"));
}

// tests/uri.test.ts:857
#[test]
fn is_disk_path_root2() {
    assert!(is_rooted_disk_uri("c:/"));
}

// tests/uri.test.ts:861
#[test]
fn is_disk_path_root3() {
    assert!(is_rooted_disk_uri("c:"));
}

// tests/uri.test.ts:865
#[test]
fn is_disk_path_root4() {
    assert!(!is_rooted_disk_uri("c:d"));
}

// tests/uri.test.ts:869
fn get_relative_path_helper(parent: &str, child: &str) -> Option<String> {
    parse_uri(parent).get_relative_path(parse_uri(child))
}

// tests/uri.test.ts:873
#[test]
fn get_relative_path() {
    assert_eq!(
        get_relative_path_helper("/a/b/c", "/a/b/c/d/e/f").as_deref(),
        Some("./d/e/f")
    );
    assert_eq!(get_relative_path_helper("/a/b/c/d/e/f", "/a/b/c/"), None);
    assert_eq!(get_relative_path_helper("/a/b/c", "/d/e/f"), None);
}

// tests/uri.test.ts:879
#[test]
#[ignore = "the in-memory vfs TestFileSystem is #[cfg(test)] inside the lib, unreachable from integration tests"]
fn case_sensitivity() {
    // const fsCaseInsensitive = new vfs.TestFileSystem(/*ignoreCase*/ true, { cwd: '/' });
    // assert.equal(fsCaseInsensitive.isLocalFileSystemCaseSensitive(), false);
    // const fsCaseSensitive = new vfs.TestFileSystem(/*ignoreCase*/ false, { cwd: '/' });
    // assert.equal(fsCaseSensitive.isLocalFileSystemCaseSensitive(), true);
    panic!("vfs.TestFileSystem is not available to integration tests");
}

// tests/uri.test.ts:889
#[test]
#[ignore = "deduplicateFolders is not ported (common/uri/uri_utils.rs); only the language server's watcher calls it"]
fn deduplicate_folders() {
    // See tests/uri.test.ts:889 for the folder lists and the expected result.
    panic!("deduplicate_folders is not ported");
}

// tests/uri.test.ts:914
#[test]
fn convert_unc_path() {
    let path = uri_ex_file("file:///server/c$/folder/file.py");

    // When converting UNC path, server part shouldn't be removed.
    assert!(path.get_path().find("server").is_some_and(|i| i > 0));
}

// tests/uri.test.ts:921
#[test]
#[ignore = "isSameUriFile is not ported (common/uri/uri_utils.rs); only the language server calls it"]
fn is_same_uri_file_file_scheme() {
    // const a = Uri.parse_uri('file:///a/b/c.py', caseDetector);
    // const b = Uri.parse_uri('file:///a/b/c.py', caseDetector);
    // const c = Uri.parse_uri('file:///a/b/d.py', caseDetector);
    // assert.strictEqual(isSameUriFile(a, b), true);
    // assert.strictEqual(isSameUriFile(a, c), false);
    panic!("is_same_uri_file is not ported");
}

// tests/uri.test.ts:930
#[test]
#[ignore = "isSameUriFile is not ported (common/uri/uri_utils.rs), and WebUri is not ported"]
fn is_same_uri_file_virtual_scheme_differing_authority() {
    panic!("is_same_uri_file is not ported");
}

// tests/uri.test.ts:940
#[test]
#[ignore = "isSameUriFile is not ported (common/uri/uri_utils.rs), and WebUri is not ported"]
fn is_same_uri_file_virtual_scheme_differing_fragment() {
    panic!("is_same_uri_file is not ported");
}

// tests/uri.test.ts:953
#[test]
#[ignore = "isSameUriFile is not ported (common/uri/uri_utils.rs), and WebUri is not ported"]
fn is_same_uri_file_virtual_scheme_differing_query() {
    panic!("is_same_uri_file is not ported");
}

// tests/uri.test.ts:960
#[test]
#[ignore = "isSameUriFile is not ported (common/uri/uri_utils.rs), and WebUri is not ported"]
fn is_same_uri_file_differing_virtual_schemes() {
    panic!("is_same_uri_file is not ported");
}

// tests/uri.test.ts:968
#[test]
#[ignore = "isSameUriFile is not ported (common/uri/uri_utils.rs), and WebUri is not ported"]
fn is_same_uri_file_mixed_file_and_virtual_scheme() {
    panic!("is_same_uri_file is not ported");
}

// tests/uri.test.ts:982
#[test]
fn realcase() {
    let temp_file = leak(RealTempFile::new());
    let fs = create_from_real_file_system(temp_file, leak(NullConsole::default()));
    // TS: `process.cwd()`, which jest sets to packages/pyright-internal.
    let cwd = pyright_internal_dir();
    let dir = Uri::file(
        &node_path::normalize(&format!("{cwd}/src/tests/../tests")),
        temp_file,
        false,
    );
    let dir_file_path = dir.get_file_path();
    let entries: Vec<String> = node_readdir_sync(dir_file_path)
        .iter()
        .map(|entry| {
            node_path::basename(&node_realpath_sync(&node_path_join(dir_file_path, entry)))
                .to_string()
        })
        .collect();
    // `lowerCaseDrive` changes nothing off win32.
    let normalized_entries = entries.clone();
    let fsentries = fs.readdir_sync(dir).unwrap();
    assert_eq!(normalized_entries, fsentries);

    let paths: Vec<String> = entries
        .iter()
        .map(|entry| node_realpath_sync(&node_path_join(dir_file_path, entry)))
        .collect();
    let fspaths: Vec<&str> = fsentries
        .iter()
        .map(|entry| {
            fs.real_case_path(dir.combine_paths(&[entry]))
                .get_file_path()
        })
        .collect();
    assert_eq!(paths, fspaths);

    // Check that the '..' has been removed.
    assert!(!fspaths.iter().any(|p| p.contains("..")));

    // The win32-only case check does not run on a POSIX host. `tempFile.dispose()`
    // has nothing to release: the port's RealTempFile creates no temp files.
}

// tests/uri.test.ts:1013
#[test]
fn realcase_use_cwd_implicitly() {
    let temp_file = leak(RealTempFile::new());
    let fs = create_from_real_file_system(temp_file, leak(NullConsole::default()));
    // TS: `process.cwd()`, which jest sets to packages/pyright-internal.
    let cwd = pyright_internal_dir();
    let dir = node_path::normalize(&format!("{cwd}/src/tests"));
    let uri = Uri::file(&dir, temp_file, false);

    let entries: Vec<String> = node_readdir_sync(&dir)
        .iter()
        .map(|entry| {
            node_path::basename(&node_realpath_sync(&node_path_join(&dir, entry))).to_string()
        })
        .collect();
    let fsentries = fs.readdir_sync(uri).unwrap();
    let paths: Vec<String> = entries
        .iter()
        .map(|entry| node_realpath_sync(&node_path_join(&dir, entry)))
        .collect();

    let fspaths: Vec<&str> = fsentries
        .iter()
        .map(|entry| {
            fs.real_case_path(uri.combine_paths(&[entry]))
                .get_file_path()
        })
        .collect();
    assert_eq!(paths, fspaths);
}

// tests/uri.test.ts:1029
#[test]
#[ignore = "WebUri is not ported: Uri::parse panics on a non-file scheme (common/uri/uri.rs)"]
fn web_uris_dont_exist() {
    let temp_file = leak(RealTempFile::new());
    let fs = create_from_real_file_system(temp_file, leak(NullConsole::default()));
    let uri = uri_ex_parse("http://www.bing.com");
    assert!(!fs.exists_sync(uri));
    let stat = fs.stat_sync(uri).unwrap();
    assert!(!stat.is_file);
}

// tests/uri.test.ts:1039
#[test]
#[ignore = "makeDirectories and FileSystem.mkdirSync are not ported: the check never writes (common/file_system.rs)"]
fn make_directories_tolerates_stale_exists_sync_results_for_existing_directories() {
    panic!("make_directories is not ported");
}

// tests/uri.test.ts:1087
#[test]
fn root_test() {
    let uri1 = uri_ex_file("C:\\");
    let uri2 = uri_ex_file("C:");
    let uri3 = uri_ex_file("/");

    assert_eq!(uri1.get_file_path(), normalize_slashes("c:/"));
    assert_eq!(uri2.get_file_path(), normalize_slashes("c:/"));
    assert_eq!(uri3.get_file_path(), normalize_slashes("/"));
}
