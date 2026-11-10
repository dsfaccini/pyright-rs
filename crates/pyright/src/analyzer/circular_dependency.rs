//! Port of `packages/pyright-internal/src/analyzer/circularDependency.ts`.

use std::cmp::Ordering;

use jsrt::JsStr;

use crate::common::uri::Uri;

// analyzer/circularDependency.ts:15
#[derive(Clone, Default)]
pub struct CircularDependency {
    paths: Vec<Uri>,
}

impl CircularDependency {
    // analyzer/circularDependency.ts:18
    pub fn append_path(&mut self, path: Uri) {
        self.paths.push(path);
    }

    // analyzer/circularDependency.ts:22
    pub fn get_paths(&self) -> &[Uri] {
        &self.paths
    }

    // analyzer/circularDependency.ts:26
    /// `path < this._paths[firstIndex]` compares two Uri objects, so JS
    /// compares their `toString()` values.
    pub fn normalize_order(&mut self) {
        let mut first_index = 0;
        for (index, path) in self.paths.iter().enumerate() {
            let first = self.paths[first_index].to_uri_string();
            if path.to_uri_string().as_str().js_cmp(first.as_str()) == Ordering::Less {
                first_index = index;
            }
        }

        if first_index != 0 {
            self.paths.rotate_left(first_index);
        }
    }

    // analyzer/circularDependency.ts:41
    /// Compares the paths by identity, as `!==` does.
    pub fn is_equal(&self, circ_dependency: &CircularDependency) -> bool {
        if circ_dependency.paths.len() != self.paths.len() {
            return false;
        }

        self.paths.iter().zip(&circ_dependency.paths).all(|(a, b)| Uri::ptr_eq(*a, *b))
    }
}
