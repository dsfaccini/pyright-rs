//! Port of `packages/pyright-internal/src/common/uri/fileUri.ts`.

use jsrt::JsStr;
use jsrt::vscode_uri::URI;
use jsrt::{FxHashMap, P};

use crate::common::path_utils::{
    ensure_trailing_directory_separator, get_directory_path, get_file_extension, get_file_name,
    get_path_components, get_root_length, has_trailing_directory_separator, is_disk_path_root,
    resolve_paths, strip_file_extension,
};
use crate::common::uri::base_uri::combine_path_elements;
use crate::common::uri::uri::{Uri, UriData, UriKind};

// common/uri/fileUri.ts:32
pub struct FileUriData {
    pub(crate) file_path: String,
    pub(crate) query: String,
    pub(crate) fragment: String,
    pub(crate) original_string: Option<String>,
    pub(crate) is_case_sensitive: bool,
}

type CacheKey = (String, String, String, Option<String>, bool);

thread_local! {
    /// `@cacheStaticFunc` on `createFileUri`. Pyright keeps the newest 8192
    /// entries; this cache keeps all of them. Equal arguments give an equal
    /// Uri either way, and no caller compares Uris by identity.
    static FILE_URI_CACHE: std::cell::RefCell<FxHashMap<CacheKey, Uri>> = Default::default();
}

// common/uri/fileUri.ts:84
pub fn create_file_uri(
    file_path: &str,
    query: &str,
    fragment: &str,
    original_string: Option<&str>,
    is_case_sensitive: bool,
) -> Uri {
    // The memoization key stringifies `undefined` as an empty string.
    let cache_key: CacheKey = (
        file_path.to_string(),
        query.to_string(),
        fragment.to_string(),
        original_string
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        is_case_sensitive,
    );
    if let Some(uri) = FILE_URI_CACHE.with(|c| c.borrow().get(&cache_key).copied()) {
        return uri;
    }

    let file_path = if is_disk_path_root(file_path) {
        ensure_trailing_directory_separator(file_path)
    } else {
        file_path.to_string()
    };

    let key = create_key(&file_path, query, fragment);
    let key = if is_case_sensitive {
        key
    } else {
        key.js_to_lower_case()
    };
    let data = FileUriData {
        file_path,
        query: query.to_string(),
        fragment: fragment.to_string(),
        original_string: original_string.map(str::to_string),
        is_case_sensitive,
    };
    let uri = Uri(P::new(UriData::new(key, UriKind::File(data))));
    FILE_URI_CACHE.with(|c| c.borrow_mut().insert(cache_key, uri));
    uri
}

// common/uri/fileUri.ts:316
fn create_key(file_path: &str, query: &str, fragment: &str) -> String {
    let mut key = file_path.to_string();
    if !query.is_empty() {
        key.push('?');
        key.push_str(query);
    }
    if !fragment.is_empty() {
        key.push('#');
        key.push_str(fragment);
    }
    key
}

impl Uri {
    fn file_ref(&self) -> &'static FileUriData {
        self.file_data().expect("file Uri")
    }

    fn create_sibling(&self, file_path: &str, query: &str, fragment: &str) -> Uri {
        create_file_uri(file_path, query, fragment, None, self.is_case_sensitive())
    }

    // common/uri/fileUri.ts:61
    pub fn file_name(&self) -> &'static str {
        match self.file_data() {
            Some(f) => self
                .0
                .get()
                .file_name
                .get_or_init(|| get_file_name(&f.file_path).to_string()),
            None => "",
        }
    }

    // common/uri/fileUri.ts:66
    pub fn last_extension(&self) -> &'static str {
        match self.file_data() {
            Some(f) => self
                .0
                .get()
                .last_extension
                .get_or_init(|| get_file_extension(&f.file_path, false)),
            None => "",
        }
    }

    // common/uri/fileUri.ts:71
    pub fn root(&self) -> Uri {
        let Some(f) = self.file_data() else {
            return *self;
        };
        *self.0.get().root.get_or_init(|| {
            let root_path = self.get_root_path();
            if root_path != f.file_path {
                self.create_sibling(&root_path, "", "")
            } else {
                *self
            }
        })
    }

    // common/uri/fileUri.ts:134
    pub(crate) fn file_to_string(&self) -> &'static str {
        let f = self.file_ref();
        self.0.get().formatted_string.get_or_init(|| {
            match f.original_string.as_deref().filter(|s| !s.is_empty()) {
                Some(original) => original.to_string(),
                None => URI::file(&f.file_path)
                    .with_query_fragment(&f.query, &f.fragment)
                    .to_string_encoded(),
            }
        })
    }

    // common/uri/fileUri.ts:147
    pub fn add_path(&self, extra: &str) -> Uri {
        match self.file_data() {
            Some(f) => self.create_sibling(&format!("{}{extra}", f.file_path), "", ""),
            None => *self,
        }
    }

    // common/uri/fileUri.ts:151
    pub fn is_root(&self) -> bool {
        self.file_data()
            .is_some_and(|f| is_disk_path_root(&f.file_path))
    }

    // common/uri/fileUri.ts:155
    pub fn is_child(&self, parent: Uri) -> bool {
        let (Some(f), Some(p)) = (self.file_data(), parent.file_data()) else {
            return false;
        };

        p.file_path.len() < f.file_path.len() && self.starts_with(Some(parent))
    }

    // common/uri/fileUri.ts:163
    pub fn is_local(&self) -> bool {
        self.file_data().is_some()
    }

    // common/uri/fileUri.ts:167
    pub fn starts_with(&self, other: Option<Uri>) -> bool {
        let (Some(f), Some(other)) = (self.file_data(), other) else {
            return false;
        };
        let Some(o) = other.file_data() else {
            return false;
        };
        if f.file_path.len() >= o.file_path.len() {
            let other_path = if f.file_path.len() > o.file_path.len()
                && !has_trailing_directory_separator(&o.file_path)
            {
                ensure_trailing_directory_separator(&o.file_path)
            } else {
                o.file_path.clone()
            };

            if !self.is_case_sensitive() {
                return f
                    .file_path
                    .js_to_lower_case()
                    .starts_with(&other_path.js_to_lower_case());
            }
            return f.file_path.starts_with(&other_path);
        }
        false
    }

    // common/uri/fileUri.ts:201
    pub fn resolve_paths(&self, paths: &[&str]) -> Uri {
        let Some(f) = self.file_data() else {
            return *self;
        };
        let mut combined = resolve_paths(&f.file_path, paths);

        if has_trailing_directory_separator(&combined) && combined.len() > 1 {
            combined.pop();
        }
        if combined != f.file_path {
            return self.create_sibling(&combined, "", "");
        }
        *self
    }

    // common/uri/fileUri.ts:215
    pub fn combine_paths(&self, paths: &[&str]) -> Uri {
        if self.file_data().is_none() {
            return *self;
        }
        if paths.len() == 1 {
            let segment = paths[0];
            if !segment.is_empty()
                && segment != "."
                && !segment.contains("..")
                && !segment.contains('/')
            {
                if let Some(cached) = self.0.get().combine_children.borrow().get(segment) {
                    return *cached;
                }

                let child = self.combine_paths_unsafe(&[segment]);
                self.0
                    .get()
                    .combine_children
                    .borrow_mut()
                    .insert(segment.to_string(), child);
                return child;
            }
        }

        if paths
            .iter()
            .any(|p| p.contains("..") || p.contains('/') || *p == ".")
        {
            return self.resolve_paths(paths);
        }

        self.combine_paths_unsafe(paths)
    }

    // common/uri/fileUri.ts:250
    pub fn combine_paths_unsafe(&self, paths: &[&str]) -> Uri {
        let Some(f) = self.file_data() else {
            return *self;
        };
        let combined = combine_path_elements(&f.file_path, "/", paths);
        if combined != f.file_path {
            return self.create_sibling(&combined, "", "");
        }
        *self
    }

    // common/uri/fileUri.ts:261
    pub fn get_directory(&self) -> Uri {
        let Some(f) = self.file_data() else {
            return *self;
        };
        *self.0.get().directory.get_or_init(|| {
            let file_path = &f.file_path;
            let mut dir = get_directory_path(file_path);
            if has_trailing_directory_separator(&dir) && dir.len() > 1 {
                dir.pop();
            }
            if dir != *file_path {
                self.create_sibling(&dir, "", "")
            } else {
                *self
            }
        })
    }

    // common/uri/fileUri.ts:274
    pub fn with_fragment(&self, fragment: &str) -> Uri {
        match self.file_data() {
            Some(f) => self.create_sibling(&f.file_path, &f.query, fragment),
            None => *self,
        }
    }

    // common/uri/fileUri.ts:278
    pub fn with_query(&self, query: &str) -> Uri {
        match self.file_data() {
            Some(f) => self.create_sibling(&f.file_path, query, &f.fragment),
            None => *self,
        }
    }

    // common/uri/fileUri.ts:282
    pub fn strip_extension(&self) -> Uri {
        let Some(f) = self.file_data() else {
            return *self;
        };
        let stripped = strip_file_extension(&f.file_path, false);
        if stripped != f.file_path {
            return self.create_sibling(&stripped, &f.query, &f.fragment);
        }
        *self
    }

    // common/uri/fileUri.ts:290
    pub fn strip_all_extensions(&self) -> Uri {
        let Some(f) = self.file_data() else {
            return *self;
        };
        let stripped = strip_file_extension(&f.file_path, true);
        if stripped != f.file_path {
            return self.create_sibling(&stripped, &f.query, &f.fragment);
        }
        *self
    }

    // common/uri/fileUri.ts:298
    pub(crate) fn get_path_components_impl(&self) -> Vec<String> {
        let Some(f) = self.file_data() else {
            return Vec::new();
        };
        let mut components = get_path_components(&f.file_path);
        if !components.is_empty() && components[0].is_empty() {
            components.remove(0);
        }
        components
            .into_iter()
            .map(|c| normalize_backslashes(&c))
            .collect()
    }

    // common/uri/fileUri.ts:308
    pub(crate) fn get_root_path(&self) -> String {
        match self.file_data() {
            Some(f) => f.file_path[..get_root_length(&f.file_path)].to_string(),
            None => String::new(),
        }
    }

    // common/uri/fileUri.ts:312
    pub(crate) fn get_comparable_path(&self) -> &'static str {
        self.get_path()
    }

    // common/uri/fileUri.ts:320
    pub(crate) fn normalized_path(&self) -> &'static str {
        let f = self.file_ref();
        self.0
            .get()
            .normalized_path
            .get_or_init(|| normalize_backslashes(&f.file_path))
    }
}

// common/uri/baseUri.ts:248
fn normalize_backslashes(path: &str) -> String {
    if path.contains('\\') {
        path.replace('\\', "/")
    } else {
        path.to_string()
    }
}
