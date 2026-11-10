//! Port of `packages/pyright-internal/src/common/uri/uri.ts`, plus the
//! `uriInterface.ts` surface. `Uri` is one handle type for every Uri class the
//! CLI creates: `FileUri`, `EmptyUri` and `ConstantUri`. `WebUri` is not
//! ported: the CLI only sees file paths.

use jsrt::JsStr;
use std::cell::{OnceCell, RefCell};
use std::fmt;
use std::hash::{Hash, Hasher};

use jsrt::vscode_uri::{self, URI};
use jsrt::{FxHashMap, P};
use regex::Regex;

use crate::common::case_sensitivity_detector::CaseSensitivityDetector;
use crate::common::path_utils::{combine_paths, is_rooted_disk_path, normalize_slashes};
use crate::common::uri::file_uri::{FileUriData, create_file_uri};

pub(crate) enum UriKind {
    File(FileUriData),
    /// `ConstantUri`; `EmptyUri` is the constant with key `<empty>`.
    Constant {
        is_empty: bool,
    },
}

pub struct UriData {
    pub(crate) key: String,
    pub(crate) kind: UriKind,
    pub(crate) file_name: OnceCell<String>,
    pub(crate) last_extension: OnceCell<String>,
    pub(crate) root: OnceCell<Uri>,
    pub(crate) directory: OnceCell<Uri>,
    pub(crate) package_uri: OnceCell<Uri>,
    pub(crate) package_stub_uri: OnceCell<Uri>,
    pub(crate) init_py_uri: OnceCell<Uri>,
    pub(crate) init_pyi_uri: OnceCell<Uri>,
    pub(crate) pytyped_uri: OnceCell<Uri>,
    pub(crate) formatted_string: OnceCell<String>,
    pub(crate) normalized_path: OnceCell<String>,
    /// `FileUri._combineChildren`; lookup only.
    pub(crate) combine_children: RefCell<FxHashMap<String, Uri>>,
}

impl UriData {
    pub(crate) fn new(key: String, kind: UriKind) -> UriData {
        UriData {
            key,
            kind,
            file_name: OnceCell::new(),
            last_extension: OnceCell::new(),
            root: OnceCell::new(),
            directory: OnceCell::new(),
            package_uri: OnceCell::new(),
            package_stub_uri: OnceCell::new(),
            init_py_uri: OnceCell::new(),
            init_pyi_uri: OnceCell::new(),
            pytyped_uri: OnceCell::new(),
            formatted_string: OnceCell::new(),
            normalized_path: OnceCell::new(),
            combine_children: RefCell::new(FxHashMap::default()),
        }
    }
}

// common/uri/uri.ts:32
/// A Uri handle. `==` is `Uri.equals`: key equality for file Uris, identity
/// for constant Uris. Hashing uses the key, so a `Uri` can stand in for the
/// `uri.key` strings pyright uses as map keys.
#[derive(Clone, Copy)]
pub struct Uri(pub(crate) P<UriData>);

impl PartialEq for Uri {
    fn eq(&self, other: &Uri) -> bool {
        match (&self.0.kind, &other.0.kind) {
            (UriKind::File(_), UriKind::File(_)) => self.0.key == other.0.key,
            _ => P::ptr_eq(self.0, other.0),
        }
    }
}

impl Eq for Uri {}

impl Hash for Uri {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.key.hash(state);
    }
}

impl fmt::Debug for Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uri({})", self.0.key)
    }
}

thread_local! {
    static EMPTY: Uri = Uri(P::new(UriData::new("<empty>".to_string(), UriKind::Constant { is_empty: true })));
}

/// `process.cwd()`.
pub fn process_cwd() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "/".to_string())
}

// common/uri/uri.ts:38
fn get_file_path(uri: &URI) -> String {
    let mut file_path = if !uri.authority.is_empty() && uri.path == "/" {
        format!("//{}{}", uri.authority, uri.path)
    } else {
        uri.fs_path()
    };

    // `/^\/[a-zA-Z]:\//`
    let b = file_path.as_bytes();
    if b.len() >= 4 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' && b[3] == b'/' {
        file_path = file_path[1..].to_string();
    }

    file_path
}

// common/uri/uri.ts:69
fn normalize_uri(parsed: URI) -> (URI, String) {
    let final_uri = if !parsed.path.is_empty() {
        vscode_uri::resolve_path(&parsed)
    } else {
        parsed
    };
    let final_string = final_uri.to_string_encoded();
    (final_uri, final_string)
}

// common/uri/uri.ts:89
fn maybe_uri(value: &str) -> bool {
    // /^[a-zA-Z][a-zA-Z0-9+.-]*:\/?\/?/ and not /^[a-zA-Z]:\\?/
    let b = value.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphabetic() {
        return false;
    }
    let mut i = 1;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'+' | b'.' | b'-')) {
        i += 1;
    }
    let is_uri = i < b.len() && b[i] == b':';
    let is_windows = b.len() >= 2 && b[1] == b':';
    is_uri && !is_windows
}

impl Uri {
    // common/uri/uri.ts:93
    pub fn create(
        value: &str,
        detector: &dyn CaseSensitivityDetector,
        check_relative: bool,
    ) -> Uri {
        if maybe_uri(value) {
            return Uri::parse(Some(value), detector);
        }

        Uri::file(value, detector, check_relative)
    }

    // common/uri/uri.ts:109
    pub fn file(path: &str, detector: &dyn CaseSensitivityDetector, check_relative: bool) -> Uri {
        let path = if check_relative && !is_rooted_disk_path(path) {
            combine_paths(&process_cwd(), &[path])
        } else {
            path.to_string()
        };

        let (uri, str) = if path.starts_with("file:") {
            normalize_uri(URI::parse(&path))
        } else {
            normalize_uri(URI::file(&normalize_slashes(&path)))
        };

        let is_case_sensitive = detector.is_case_sensitive(&str);
        create_file_uri(
            &get_file_path(&uri),
            &uri.query,
            &uri.fragment,
            Some(&str),
            is_case_sensitive,
        )
    }

    // common/uri/uri.ts:133
    pub fn parse(uri_str: Option<&str>, detector: &dyn CaseSensitivityDetector) -> Uri {
        let Some(uri_str) = uri_str.filter(|s| !s.is_empty()) else {
            return Uri::empty();
        };

        let (uri, str) = normalize_uri(URI::parse(uri_str));
        if uri.scheme == "file" {
            let is_case_sensitive = detector.is_case_sensitive(&str);
            return create_file_uri(
                &get_file_path(&uri),
                &uri.query,
                &uri.fragment,
                Some(&str),
                is_case_sensitive,
            );
        }

        // `WebUri` is not ported; the CLI never resolves a non-file Uri.
        panic!("non-file Uri is not supported: {uri_str}");
    }

    // common/uri/uri.ts:166
    pub fn constant(marker_name: &str) -> Uri {
        Uri(P::new(UriData::new(
            marker_name.to_string(),
            UriKind::Constant { is_empty: false },
        )))
    }

    // common/uri/uri.ts:170
    pub fn empty() -> Uri {
        EMPTY.with(|e| *e)
    }

    // common/uri/uri.ts:176
    pub const DEFAULT_WORKSPACE_ROOT_COMPONENT: &'static str = "<default workspace root>";

    // common/uri/uri.ts:179
    pub fn default_workspace(detector: &dyn CaseSensitivityDetector) -> Uri {
        Uri::file("/<default workspace root>", detector, false)
    }

    // common/uri/uri.ts:211
    pub fn is_empty_opt(uri: Option<Uri>) -> bool {
        uri.is_none_or(|u| u.is_empty())
    }

    // common/uri/uri.ts:215
    pub fn equals_opt(a: Option<Uri>, b: Option<Uri>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => a.equals(Some(b)),
            (None, None) => true,
            _ => false,
        }
    }

    // common/uri/uri.ts:223
    pub fn is_default_workspace(uri: Uri) -> bool {
        uri.file_name()
            .contains(Uri::DEFAULT_WORKSPACE_ROOT_COMPONENT)
    }

    pub(crate) fn file_data(&self) -> Option<&'static FileUriData> {
        match &self.0.get().kind {
            UriKind::File(data) => Some(data),
            UriKind::Constant { .. } => None,
        }
    }

    pub fn key(&self) -> &'static str {
        &self.0.get().key
    }

    pub fn scheme(&self) -> &'static str {
        if self.file_data().is_some() {
            "file"
        } else {
            ""
        }
    }

    pub fn is_case_sensitive(&self) -> bool {
        self.file_data().is_none_or(|f| f.is_case_sensitive)
    }

    pub fn fragment(&self) -> &'static str {
        self.file_data().map_or("", |f| f.fragment.as_str())
    }

    pub fn query(&self) -> &'static str {
        self.file_data().map_or("", |f| f.query.as_str())
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.0.get().kind, UriKind::Constant { is_empty: true })
    }

    /// `a === b`: object identity.
    pub fn ptr_eq(a: Uri, b: Uri) -> bool {
        P::ptr_eq(a.0, b.0)
    }

    /// `equals(other)`.
    pub fn equals(&self, other: Option<Uri>) -> bool {
        match other {
            Some(other) => *self == other,
            None => false,
        }
    }

    /// `uri.toString()`.
    pub fn to_uri_string(&self) -> String {
        match &self.0.get().kind {
            UriKind::File(_) => self.file_to_string().to_string(),
            UriKind::Constant { is_empty: true } => String::new(),
            UriKind::Constant { is_empty: false } => self.key().to_string(),
        }
    }

    pub fn to_user_visible_string(&self) -> &'static str {
        self.file_data().map_or("", |f| f.file_path.as_str())
    }

    pub fn matches_regex(&self, regex: &Regex) -> bool {
        match self.file_data() {
            Some(_) => regex.is_match(self.normalized_path()),
            None => false,
        }
    }

    pub fn get_path(&self) -> &'static str {
        match self.file_data() {
            Some(_) => self.normalized_path(),
            None => "",
        }
    }

    pub fn get_file_path(&self) -> &'static str {
        self.file_data().map_or("", |f| f.file_path.as_str())
    }

    pub fn get_path_length(&self) -> i32 {
        self.file_data().map_or(0, |f| f.file_path.js_len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::case_sensitivity_detector::CaseSensitiveDetector;

    fn file(path: &str) -> Uri {
        Uri::file(path, &CaseSensitiveDetector, false)
    }

    // Ported from tests/uri.test.ts (POSIX file cases).

    #[test]
    fn parse_empty() {
        assert!(Uri::parse(Some(""), &CaseSensitiveDetector).is_empty());
        assert!(Uri::parse(None, &CaseSensitiveDetector).is_empty());
    }

    #[test]
    fn file_key_and_string() {
        let uri = file("/a/b/c.py");
        assert_eq!(uri.key(), "/a/b/c.py");
        assert_eq!(uri.to_uri_string(), "file:///a/b/c.py");
        assert_eq!(uri.get_file_path(), "/a/b/c.py");
        assert_eq!(uri.file_name(), "c.py");
        assert_eq!(uri.last_extension(), ".py");
    }

    #[test]
    fn file_normalizes_dots_and_trailing_slash() {
        assert_eq!(file("/a/b/../c/").get_file_path(), "/a/c");
        assert_eq!(file("/").get_file_path(), "/");
        assert_eq!(file("a\\b").get_file_path(), "/a/b");
    }

    #[test]
    fn root_and_directory() {
        let uri = file("/a/b/c.py");
        assert_eq!(uri.root().get_file_path(), "/");
        assert_eq!(uri.get_directory().get_file_path(), "/a/b");
        assert_eq!(file("/a").get_directory().get_file_path(), "/");
        assert_eq!(file("/").get_directory(), file("/"));
    }

    #[test]
    fn combine_paths() {
        let uri = file("/a/b");
        assert_eq!(
            uri.combine_paths(&["c", "d.py"]).get_file_path(),
            "/a/b/c/d.py"
        );
        assert_eq!(uri.combine_paths(&["../c"]).get_file_path(), "/a/c");
        assert_eq!(uri.combine_paths(&["/x"]).get_file_path(), "/x");
        assert_eq!(uri.resolve_paths(&["..", "x/"]).get_file_path(), "/a/x");
    }

    #[test]
    fn starts_with_and_child() {
        let parent = file("/a/b");
        let child = file("/a/b/c");
        assert!(child.starts_with(Some(parent)));
        assert!(child.is_child(parent));
        assert!(!file("/a/bc").starts_with(Some(parent)));
        assert!(!parent.is_child(parent));
        assert_eq!(parent.get_relative_path(child).as_deref(), Some("./c"));
    }

    #[test]
    fn extensions() {
        let uri = file("/a/b.tar.gz");
        assert_eq!(uri.strip_extension().get_file_path(), "/a/b.tar");
        assert_eq!(uri.strip_all_extensions().get_file_path(), "/a/b");
        assert_eq!(uri.file_name_without_extensions(), "b.tar");
        assert!(uri.has_extension(".gz"));
        assert!(uri.contains_extension(".tar"));
        assert_eq!(file("/a/b").package_uri().get_file_path(), "/a/b.py");
        assert_eq!(
            file("/a/b").init_pyi_uri().get_file_path(),
            "/a/b/__init__.pyi"
        );
        assert_eq!(
            file("/a/b.py").replace_extension(".pyi").get_file_path(),
            "/a/b.pyi"
        );
    }

    #[test]
    fn path_components() {
        assert_eq!(
            file("/a/b/c").get_path_components(),
            vec!["/", "a", "b", "c"]
        );
        assert_eq!(
            file("/a/b").get_relative_path_components(file("/a/c/d")),
            vec!["..", "c", "d"]
        );
    }

    #[test]
    fn encoded_string_orders_like_pyright() {
        assert_eq!(file("/a b/c").to_uri_string(), "file:///a%20b/c");
    }

    #[test]
    fn empty_and_constant_identity() {
        assert_eq!(Uri::empty(), Uri::empty());
        let c = Uri::constant("x");
        assert_ne!(c, Uri::constant("x"));
        assert_eq!(c, c);
    }
}
