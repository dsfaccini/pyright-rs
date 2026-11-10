//! Port of `packages/pyright-internal/src/partialStubService.ts`.
//!
//! `SupportPartialStubs.is` (duck typing for `createServiceProvider`) is not
//! ported. No caller passes `processPartialStubPackages`'s `allowMoving`, so
//! it always uses `_allowMoving`.

use std::cell::RefCell;

use jsrt::FxHashSet;

use crate::analyzer::py_typed_utils::{PyTypedInfo, get_py_typed_info};
use crate::common::config_options::ExecutionEnvironment;
use crate::common::file_system::{Disposable, FileSystem};
use crate::common::path_consts::STUBS_SUFFIX;
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::{is_directory, try_stat};

// partialStubService.ts:19
pub trait SupportPartialStubs {
    fn is_partial_stub_packages_scanned(&self, exec_env: &ExecutionEnvironment) -> bool;
    fn is_path_scanned(&self, path: Uri) -> bool;
    fn process_partial_stub_packages(
        &self,
        paths: &[Uri],
        roots: &[Uri],
        bundled_stub_path: Option<Uri>,
    );
    fn clear_partial_stubs(&self);
}

// partialStubService.ts:37
#[derive(jsrt::Trace)]
pub struct PartialStubService {
    // Root paths processed
    _root_searched: RefCell<FxHashSet<Uri>>,

    // Partial stub package paths processed
    _partial_stub_package_paths: RefCell<FxHashSet<Uri>>,

    // Disposables to cleanup moved directories
    _moved_directories: RefCell<Vec<Disposable>>,

    _real_fs: &'static dyn FileSystem,
}

impl PartialStubService {
    // partialStubService.ts:47
    pub fn new(real_fs: &'static dyn FileSystem) -> PartialStubService {
        PartialStubService {
            _root_searched: RefCell::new(FxHashSet::default()),
            _partial_stub_package_paths: RefCell::new(FxHashSet::default()),
            _moved_directories: RefCell::new(Vec::new()),
            _real_fs: real_fs,
        }
    }

    // partialStubService.ts:143
    fn _allow_moving(
        &self,
        is_bundled: bool,
        package_py_typed: Option<PyTypedInfo>,
        _stub_py_typed: PyTypedInfo,
    ) -> bool {
        if !is_bundled {
            return true;
        }

        // If partial stub we found is from bundled stub and library installed is marked as py.typed
        // allow moving only if the package is marked as partially typed.
        package_py_typed.is_none_or(|p| p.is_partially_typed)
    }
}

impl SupportPartialStubs for PartialStubService {
    // partialStubService.ts:51
    fn is_partial_stub_packages_scanned(&self, exec_env: &ExecutionEnvironment) -> bool {
        match exec_env.root {
            Some(root) => self.is_path_scanned(root),
            None => false,
        }
    }

    // partialStubService.ts:55
    fn is_path_scanned(&self, uri: Uri) -> bool {
        self._root_searched.borrow().contains(&uri)
    }

    // partialStubService.ts:59
    fn process_partial_stub_packages(
        &self,
        paths: &[Uri],
        roots: &[Uri],
        bundled_stub_path: Option<Uri>,
    ) {
        for &path in paths {
            self._root_searched.borrow_mut().insert(path);

            if !self._real_fs.exists_sync(path) || !is_directory(self._real_fs, path) {
                continue;
            }

            // Leave empty set of dir entries to process when reading fails.
            let dir_entries = self._real_fs.readdir_entries_sync(path).unwrap_or_default();

            let is_bundled_stub = path.equals(bundled_stub_path);
            for entry in &dir_entries {
                let partial_stub_package_path = path.combine_paths(&[&entry.name]);
                let is_directory = if !entry.is_symbolic_link {
                    entry.is_directory
                } else {
                    try_stat(self._real_fs, partial_stub_package_path)
                        .is_some_and(|s| s.is_directory)
                };

                if !is_directory || !entry.name.ends_with(STUBS_SUFFIX) {
                    continue;
                }

                let py_typed_info = get_py_typed_info(self._real_fs, partial_stub_package_path);
                let Some(py_typed_info) = py_typed_info.filter(|p| p.is_partially_typed) else {
                    // Stub-Package is fully typed.
                    continue;
                };

                // We found partially typed stub-packages.
                self._partial_stub_package_paths
                    .borrow_mut()
                    .insert(partial_stub_package_path);

                // Search the root to see whether we have matching package installed.
                let package_name = &entry.name[..entry.name.len() - STUBS_SUFFIX.len()];
                for root in roots {
                    let package_path = root.combine_paths(&[package_name]);
                    let stat = try_stat(self._real_fs, package_path);
                    if !stat.is_some_and(|s| s.is_directory) {
                        continue;
                    }

                    // If partial stub we found is from bundled stub and library installed is marked as py.typed
                    // ignore bundled partial stub.
                    if !self._allow_moving(
                        is_bundled_stub,
                        get_py_typed_info(self._real_fs, package_path),
                        py_typed_info,
                    ) {
                        continue;
                    }
                    // Merge partial stub packages to the library.
                    let moved = self._real_fs.map_directory(
                        package_path,
                        partial_stub_package_path,
                        Some(Box::new(|u: Uri, fs: &dyn FileSystem| {
                            u.has_extension(".pyi")
                                || (fs.exists_sync(u)
                                    && fs.stat_sync(u).is_ok_and(|s| s.is_directory))
                        })),
                    );
                    self._moved_directories.borrow_mut().push(moved);
                }
            }
        }
    }

    // partialStubService.ts:136
    fn clear_partial_stubs(&self) {
        self._root_searched.borrow_mut().clear();
        self._partial_stub_package_paths.borrow_mut().clear();
        let moved_directories = std::mem::take(&mut *self._moved_directories.borrow_mut());
        moved_directories.iter().for_each(|d| d.dispose());
    }
}

// partialStubService.ts:163
/// A no-op implementation of `SupportPartialStubs` for tests that don't need
/// partial stub package resolution.
pub struct NoOpPartialStubs;

impl SupportPartialStubs for NoOpPartialStubs {
    // partialStubService.ts:164
    fn is_partial_stub_packages_scanned(&self, _exec_env: &ExecutionEnvironment) -> bool {
        // Always report as already scanned to prevent actual scanning.
        true
    }

    // partialStubService.ts:169
    fn is_path_scanned(&self, _path: Uri) -> bool {
        true
    }

    // partialStubService.ts:173
    fn process_partial_stub_packages(
        &self,
        _paths: &[Uri],
        _roots: &[Uri],
        _bundled_stub_path: Option<Uri>,
    ) {
        // No-op: skip all file system operations.
    }

    // partialStubService.ts:177
    fn clear_partial_stubs(&self) {
        // No-op.
    }
}
