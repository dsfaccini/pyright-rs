//! Port of `packages/pyright-internal/src/tests/harness/testAccessHost.ts`.

use crate::analyzer::import_logger::ImportLogger;
use crate::analyzer::python_path_utils::PythonPathResult;
use crate::common::config_options::PythonPlatform;
use crate::common::file_system::FileSystem;
use crate::common::host::{Host, HostKind};
use crate::common::python_version::PythonVersion;
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::is_directory;

// tests/harness/testAccessHost.ts:16
/// `extends NoAccessHost`.
pub struct TestAccessHost {
    _module_path: Uri,
    _search_paths: Vec<Uri>,
    _fs: Option<&'static dyn FileSystem>,
}

impl TestAccessHost {
    // tests/harness/testAccessHost.ts:17
    pub fn new(
        module_path: Uri,
        search_paths: Vec<Uri>,
        fs: Option<&'static dyn FileSystem>,
    ) -> TestAccessHost {
        TestAccessHost {
            _module_path: module_path,
            _search_paths: search_paths,
            _fs: fs,
        }
    }

    /// A `TestAccessHost` that lives for the rest of the test thread.
    pub fn leak(
        module_path: Uri,
        search_paths: Vec<Uri>,
        fs: Option<&'static dyn FileSystem>,
    ) -> &'static TestAccessHost {
        Box::leak(Box::new(TestAccessHost::new(module_path, search_paths, fs)))
    }
}

impl Host for TestAccessHost {
    fn kind(&self) -> HostKind {
        HostKind::NoAccess
    }

    // tests/harness/testAccessHost.ts:21
    fn get_python_search_paths(
        &self,
        _python_path: Option<Uri>,
        _import_logger: Option<&ImportLogger>,
        _cwd: Option<Uri>,
    ) -> PythonPathResult {
        // Filter out non-directory paths if filesystem is available
        let filtered_paths = match self._fs {
            Some(fs) => self
                ._search_paths
                .iter()
                .copied()
                .filter(|p| is_directory(fs, *p))
                .collect(),
            None => self._search_paths.clone(),
        };

        PythonPathResult {
            paths: filtered_paths,
            prefix: Some(self._module_path),
        }
    }

    fn get_python_version(
        &self,
        _python_path: Option<Uri>,
        _import_logger: Option<&ImportLogger>,
    ) -> Option<PythonVersion> {
        None
    }

    fn get_python_platform(&self, _import_logger: Option<&ImportLogger>) -> Option<PythonPlatform> {
        None
    }
}
