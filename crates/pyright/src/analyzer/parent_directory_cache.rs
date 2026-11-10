//! Port of `packages/pyright-internal/src/analyzer/parentDirectoryCache.ts`.
//!
//! Cache to hold parent directory import result to make sure
//! we don't repeatedly search folders.

use std::cell::{Cell, RefCell};

use jsrt::{JsMap, P};

use crate::analyzer::import_result::ImportResult;
use crate::common::file_system::ReadOnlyFileSystem;
use crate::common::uri::Uri;

// analyzer/parentDirectoryCache.ts:15
/// Shared: every directory walked for one import stores the same object, and
/// the walk writes `import_path` once it finds the module.
pub struct ImportPath {
    pub import_path: Cell<Option<Uri>>,
}

// analyzer/parentDirectoryCache.ts:17
pub struct CacheEntry {
    pub import_result: P<ImportResult>,
    pub path: Uri,
    pub import_name: String,
}

// analyzer/parentDirectoryCache.ts:19
/// pyright passes `_importRootGetter` to the constructor; it reads the
/// resolver that owns this cache, so `check_valid_path` takes it instead.
pub struct ParentDirectoryCache {
    _import_checked: RefCell<JsMap<String, JsMap<Uri, P<ImportPath>>>>,
    _cached_results: RefCell<JsMap<String, JsMap<Uri, P<ImportResult>>>>,

    _lib_path_cache: RefCell<Option<Vec<Uri>>>,
}

impl ParentDirectoryCache {
    // analyzer/parentDirectoryCache.ts:25
    pub fn new() -> ParentDirectoryCache {
        ParentDirectoryCache {
            _import_checked: RefCell::new(JsMap::default()),
            _cached_results: RefCell::new(JsMap::default()),
            _lib_path_cache: RefCell::new(None),
        }
    }

    // analyzer/parentDirectoryCache.ts:29
    pub fn get_import_result(
        &self,
        path: Uri,
        import_name: &str,
        import_result: P<ImportResult>,
    ) -> Option<P<ImportResult>> {
        let result = self
            ._cached_results
            .borrow()
            .get(import_name)
            .and_then(|m| m.get(&path))
            .copied();
        if let Some(result) = result {
            // We already checked for the importName at the path.
            return Some(result);
        }

        let checked = self
            ._import_checked
            .borrow()
            .get(import_name)
            .and_then(|m| m.get(&path))
            .copied();
        if let Some(checked) = checked {
            // We already checked for the importName at the path.
            let Some(import_path) = checked.import_path.get() else {
                return Some(import_result);
            };

            let cached = self
                ._cached_results
                .borrow()
                .get(import_name)
                .and_then(|m| m.get(&import_path))
                .copied();
            return Some(cached.unwrap_or(import_result));
        }

        None
    }

    // analyzer/parentDirectoryCache.ts:49
    pub fn check_valid_path(
        &self,
        fs: &dyn ReadOnlyFileSystem,
        source_file_uri: Uri,
        root: Uri,
        import_root_getter: &dyn Fn() -> Vec<Uri>,
    ) -> bool {
        if !source_file_uri.starts_with(Some(root)) {
            // We don't search containing folders for libs.
            return false;
        }

        if self._lib_path_cache.borrow().is_none() {
            let lib_path_cache: Vec<Uri> = import_root_getter()
                .into_iter()
                .map(|r| fs.real_case_path(r))
                .filter(|r| !r.equals(Some(root)))
                .filter(|r| r.starts_with(Some(root)))
                .collect();
            *self._lib_path_cache.borrow_mut() = Some(lib_path_cache);
        }

        if self
            ._lib_path_cache
            .borrow()
            .as_ref()
            .is_some_and(|c| c.iter().any(|p| source_file_uri.starts_with(Some(*p))))
        {
            // Make sure it is not lib folders under user code root.
            // ex) .venv folder
            return false;
        }

        true
    }

    // analyzer/parentDirectoryCache.ts:71
    pub fn checked(&self, path: Uri, import_name: &str, import_path: P<ImportPath>) {
        self._import_checked
            .borrow_mut()
            .entry(import_name.to_string())
            .or_default()
            .insert(path, import_path);
    }

    // analyzer/parentDirectoryCache.ts:75
    pub fn add(&self, result: CacheEntry) {
        self._cached_results
            .borrow_mut()
            .entry(result.import_name)
            .or_default()
            .insert(result.path, result.import_result);
    }

    // analyzer/parentDirectoryCache.ts:82
    pub fn reset(&self) {
        self._import_checked.borrow_mut().clear();
        self._cached_results.borrow_mut().clear();
        *self._lib_path_cache.borrow_mut() = None;
    }
}
