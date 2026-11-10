//! Port of `packages/pyright-internal/src/analyzer/importStatementUtils.ts`.
//!
//! Only `getDirectoryLeadingDotsPointsTo`, which the import resolver uses.

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
