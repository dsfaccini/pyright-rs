//! Port of `packages/pyright-internal/src/analyzer/pyTypedUtils.ts`.

use crate::common::file_system::ReadOnlyFileSystem;
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::{is_directory, is_file};

// analyzer/pyTypedUtils.ts:14
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(jsrt::Trace)]
pub struct PyTypedInfo {
    pub py_typed_path: Uri,
    pub is_partially_typed: bool,
}

// analyzer/pyTypedUtils.ts:22
pub fn get_py_typed_info(
    file_system: &dyn ReadOnlyFileSystem,
    dir_path: Uri,
) -> Option<PyTypedInfo> {
    if !file_system.exists_sync(dir_path) || !is_directory(file_system, dir_path) {
        return None;
    }

    let py_typed_path = dir_path.pytyped_uri();
    if !file_system.exists_sync(py_typed_path) || !is_file(file_system, py_typed_path, false) {
        return None;
    }

    Some(get_py_typed_info_for_py_typed_file(
        file_system,
        py_typed_path,
    ))
}

// analyzer/pyTypedUtils.ts:38
/// `statSync` and `readFileSync` throw on an unreadable file; pyright does not
/// catch it there, so the caller must pass an existing file.
pub fn get_py_typed_info_for_py_typed_file(
    file_system: &dyn ReadOnlyFileSystem,
    py_typed_path: Uri,
) -> PyTypedInfo {
    let mut is_partially_typed = false;

    let file_stats = file_system.stat_sync(py_typed_path).unwrap_or_default();

    if file_stats.size > 0 && file_stats.size < 64 * 1024 {
        let py_typed_contents = file_system
            .read_file_text_sync(py_typed_path)
            .unwrap_or_default();

        if py_typed_contents.contains("partial\n") || py_typed_contents.contains("partial\r\n") {
            is_partially_typed = true;
        }
    }

    PyTypedInfo {
        py_typed_path,
        is_partially_typed,
    }
}
