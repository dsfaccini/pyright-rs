//! Port of `packages/pyright-internal/src/common/host.ts`.
//!
//! `runScript`, `runSnippet` and `spawnProcess` serve the language server and
//! are not ported.

use crate::analyzer::import_logger::ImportLogger;
use crate::analyzer::python_path_utils::PythonPathResult;
use crate::common::config_options::PythonPlatform;
use crate::common::python_version::PythonVersion;
use crate::common::uri::Uri;

// common/host.ts:17
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostKind {
    FullAccess,
    LimitedAccess,
    NoAccess,
}

// common/host.ts:69
pub trait Host {
    fn kind(&self) -> HostKind;
    fn get_python_search_paths(
        &self,
        python_path: Option<Uri>,
        failure_logger: Option<&ImportLogger>,
        cwd: Option<Uri>,
    ) -> PythonPathResult;
    fn get_python_version(
        &self,
        python_path: Option<Uri>,
        failure_logger: Option<&ImportLogger>,
    ) -> Option<PythonVersion>;
    fn get_python_platform(&self, failure_logger: Option<&ImportLogger>) -> Option<PythonPlatform>;
}

// common/host.ts:97
pub struct NoAccessHost;

impl Host for NoAccessHost {
    fn kind(&self) -> HostKind {
        HostKind::NoAccess
    }

    // common/host.ts:102
    fn get_python_search_paths(
        &self,
        _python_path: Option<Uri>,
        failure_logger: Option<&ImportLogger>,
        _cwd: Option<Uri>,
    ) -> PythonPathResult {
        if let Some(logger) = failure_logger {
            logger.log("No access to python executable.");
        }

        PythonPathResult {
            paths: Vec::new(),
            prefix: None,
        }
    }

    // common/host.ts:111
    fn get_python_version(
        &self,
        _python_path: Option<Uri>,
        _failure_logger: Option<&ImportLogger>,
    ) -> Option<PythonVersion> {
        None
    }

    // common/host.ts:115
    fn get_python_platform(
        &self,
        _failure_logger: Option<&ImportLogger>,
    ) -> Option<PythonPlatform> {
        None
    }
}
