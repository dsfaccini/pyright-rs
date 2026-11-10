//! Port of `packages/pyright-internal/src/analyzer/importStatementUtils.ts`.
//!
//! Only `getDirectoryLeadingDotsPointsTo`, which the import resolver uses,
//! `getWildcardImportNames`, which the binder calls, and `getRelativeModuleName`,
//! which the checker calls. The rest serves the language server.

use jsrt::JsStr;

use super::analyzer_file_info::ImportLookupResult;
use crate::common::uri::Uri;

// analyzer/importStatementUtils.ts:921
/// TS defaults: ignoreFolderStructure = false.
pub fn get_relative_module_name(
    fs: &dyn crate::common::file_system::ReadOnlyFileSystem,
    source_path: Uri,
    target_path: Uri,
    config_options: &crate::common::config_options::ConfigOptions,
    ignore_folder_structure: bool,
    source_is_file: Option<bool>,
) -> Option<String> {
    let mut src_path = source_path;
    let source_is_file =
        source_is_file.unwrap_or_else(|| crate::common::uri::uri_utils::is_file(fs, source_path, false));
    if source_is_file {
        src_path = source_path.get_directory();
    }

    let mut symbol_name: Option<String> = None;
    let mut dest_path = target_path;
    if config_options.stub_path.is_some_and(|stub_path| dest_path.is_child(stub_path))
        || config_options.typeshed_path.is_some_and(|typeshed_path| dest_path.is_child(typeshed_path))
    {
        // Always use absolute imports for files in these library-like directories.
        return None;
    }
    if source_is_file {
        dest_path = target_path.get_directory();

        let file_name = target_path.strip_all_extensions().file_name();
        if file_name != "__init__" {
            // ex) src: a.py, dest: b.py -> ".b" will be returned.
            symbol_name = Some(file_name.to_string());
        } else if ignore_folder_structure {
            // ex) src: nested1/nested2/__init__.py, dest: nested1/__init__.py -> "...nested1" will be returned
            //     like how it would return for sibling folder.
            //
            // if folder structure is not ignored, ".." will be returned
            symbol_name = Some(dest_path.file_name().to_string());
            dest_path = dest_path.get_directory();
        }
    }

    let relative_paths = src_path.get_relative_path_components(dest_path);

    // This assumes both file paths are under the same importing root.
    // So this doesn't handle paths pointing to 2 different import roots.
    // ex) user file A to library file B
    let mut current_paths = ".".to_string();
    for (i, relative_path) in relative_paths.iter().enumerate() {
        if relative_path == ".." {
            current_paths += ".";
        } else {
            current_paths += relative_path;
        }

        if relative_path != ".." && i != relative_paths.len() - 1 {
            current_paths += ".";
        }
    }

    // `if (symbolName)`: an empty name is falsy.
    if let Some(symbol_name) = symbol_name.filter(|name| !name.is_empty()) {
        current_paths = if current_paths.ends_with('.') {
            current_paths + &symbol_name
        } else {
            current_paths + "." + &symbol_name
        };
    }

    Some(current_paths)
}

// analyzer/importStatementUtils.ts:988
pub fn get_directory_leading_dots_points_to(from_directory: Uri, leading_dots: i32) -> Option<Uri> {
    let mut current_directory = from_directory;
    for _ in 1..leading_dots {
        if current_directory.is_root() {
            return None;
        }

        current_directory = current_directory.get_directory();
    }

    Some(current_directory)
}

// analyzer/importStatementUtils.ts:1040
pub fn get_wildcard_import_names(lookup_info: &ImportLookupResult) -> Vec<String> {
    let mut names_to_import: Vec<String> = Vec::new();

    // If a dunder all symbol is defined, it takes precedence.
    if let Some(dunder_all_names) = &lookup_info.dunder_all_names {
        if !lookup_info.uses_unsupported_dunder_all_form {
            return dunder_all_names.clone();
        }

        names_to_import.extend(dunder_all_names.iter().cloned());
    }

    lookup_info.symbol_table.for_each(|symbol, name| {
        if !symbol.is_externally_hidden() && !name.js_starts_with("_", 0) {
            names_to_import.push(name.to_string());
        }
    });

    names_to_import
}
