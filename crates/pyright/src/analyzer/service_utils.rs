//! Port of `packages/pyright-internal/src/analyzer/serviceUtils.ts`.

use crate::common::file_system::ReadOnlyFileSystem;
use crate::common::path_consts::{CONFIG_FILE_NAME, PYPROJECT_TOML_NAME};
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::for_each_ancestor_directory;

// analyzer/serviceUtils.ts:6
pub fn find_pyproject_toml_file_here_or_up(fs: &dyn ReadOnlyFileSystem, search_path: Uri) -> Option<Uri> {
    for_each_ancestor_directory(search_path, |ancestor| find_pyproject_toml_file(fs, ancestor))
}

// analyzer/serviceUtils.ts:10
pub fn find_pyproject_toml_file(fs: &dyn ReadOnlyFileSystem, search_path: Uri) -> Option<Uri> {
    let file_name = search_path.resolve_paths(&[PYPROJECT_TOML_NAME]);
    if fs.exists_sync(file_name) {
        return Some(fs.real_case_path(file_name));
    }
    None
}

// analyzer/serviceUtils.ts:18
pub fn find_config_file_here_or_up(fs: &dyn ReadOnlyFileSystem, search_path: Uri) -> Option<Uri> {
    for_each_ancestor_directory(search_path, |ancestor| find_config_file(fs, ancestor))
}

// analyzer/serviceUtils.ts:22
pub fn find_config_file(fs: &dyn ReadOnlyFileSystem, search_path: Uri) -> Option<Uri> {
    let file_name = search_path.resolve_paths(&[CONFIG_FILE_NAME]);
    if fs.exists_sync(file_name) {
        return Some(fs.real_case_path(file_name));
    }

    None
}
