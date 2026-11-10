//! Port of `packages/pyright-internal/src/analyzer/importStatementUtils.ts`.
//!
//! Only `getDirectoryLeadingDotsPointsTo`, which the import resolver uses, and
//! `getWildcardImportNames`, which the binder calls. The rest serves the
//! language server.

use jsrt::JsStr;

use super::analyzer_file_info::ImportLookupResult;
use crate::common::uri::Uri;

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
