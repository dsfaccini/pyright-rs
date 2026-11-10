//! Port of `packages/pyright-internal/src/common/uri/baseUri.ts`.

use crate::common::path_utils::{get_root_length, get_shortened_file_name};
use crate::common::uri::uri::Uri;
use jsrt::JsStr;

impl Uri {
    // common/uri/baseUri.ts:36
    pub fn file_name_without_extensions(&self) -> &'static str {
        let file_name = self.file_name();
        match file_name.rfind('.') {
            Some(index) if index > 0 => &file_name[..index],
            _ => file_name,
        }
    }

    // common/uri/baseUri.ts:54
    pub fn package_uri(&self) -> Uri {
        *self
            .0
            .get()
            .package_uri
            .get_or_init(|| self.add_extension(".py"))
    }

    // common/uri/baseUri.ts:61
    pub fn package_stub_uri(&self) -> Uri {
        *self
            .0
            .get()
            .package_stub_uri
            .get_or_init(|| self.add_extension(".pyi"))
    }

    // common/uri/baseUri.ts:68
    pub fn init_py_uri(&self) -> Uri {
        *self
            .0
            .get()
            .init_py_uri
            .get_or_init(|| self.combine_paths_unsafe(&["__init__.py"]))
    }

    // common/uri/baseUri.ts:75
    pub fn init_pyi_uri(&self) -> Uri {
        *self
            .0
            .get()
            .init_pyi_uri
            .get_or_init(|| self.combine_paths_unsafe(&["__init__.pyi"]))
    }

    // common/uri/baseUri.ts:82
    pub fn pytyped_uri(&self) -> Uri {
        *self
            .0
            .get()
            .pytyped_uri
            .get_or_init(|| self.combine_paths_unsafe(&["py.typed"]))
    }

    // common/uri/baseUri.ts:102
    pub fn replace_extension(&self, ext: &str) -> Uri {
        let dir = self.get_directory();
        let base = self.file_name();
        let new_base = format!("{}{ext}", &base[..base.len() - self.last_extension().len()]);
        dir.combine_paths_unsafe(&[&new_base])
    }

    // common/uri/baseUri.ts:109
    pub fn add_extension(&self, ext: &str) -> Uri {
        self.add_path(ext)
    }

    // common/uri/baseUri.ts:113
    pub fn has_extension(&self, ext: &str) -> bool {
        if self.is_case_sensitive() {
            self.last_extension() == ext
        } else {
            self.last_extension().js_to_lower_case() == ext.js_to_lower_case()
        }
    }

    // common/uri/baseUri.ts:119
    pub fn contains_extension(&self, ext: &str) -> bool {
        let file_name = self.file_name();
        // `fileName.split(/(?=\.)/g)`: split before every dot.
        let mut extensions = Vec::new();
        let mut last = 0;
        for (i, _) in file_name.match_indices('.') {
            if i > 0 {
                extensions.push(&file_name[last..i]);
                last = i;
            }
        }
        extensions.push(&file_name[last..]);
        extensions.iter().any(|e| {
            if self.is_case_sensitive() {
                *e == ext
            } else {
                e.js_to_lower_case() == ext.js_to_lower_case()
            }
        })
    }

    // common/uri/baseUri.ts:134
    pub fn get_root_path_length(&self) -> usize {
        self.get_root_path().len()
    }

    // common/uri/baseUri.ts:146
    pub fn is_untitled(&self) -> bool {
        self.scheme() == "untitled"
    }

    // common/uri/baseUri.ts:156
    pub fn path_starts_with(&self, name: &str) -> bool {
        self.get_comparable_path().starts_with(name)
    }

    // common/uri/baseUri.ts:161
    pub fn path_ends_with(&self, name: &str) -> bool {
        self.get_comparable_path().ends_with(name)
    }

    // common/uri/baseUri.ts:166
    pub fn path_includes(&self, include: &str) -> bool {
        self.get_comparable_path().contains(include)
    }

    // common/uri/baseUri.ts:183
    pub fn get_relative_path(&self, child: Uri) -> Option<String> {
        if self.scheme() != child.scheme() {
            return None;
        }

        if child.is_child(*self) {
            let relative_to_components = self.get_relative_path_components(child);
            if !relative_to_components.is_empty() {
                let mut parts = vec![".".to_string()];
                parts.extend(relative_to_components);
                return Some(parts.join("/"));
            }
        }
        None
    }

    // common/uri/baseUri.ts:199
    pub fn get_path_components(&self) -> Vec<String> {
        self.get_path_components_impl()
    }

    // common/uri/baseUri.ts:208
    pub fn get_relative_path_components(&self, to: Uri) -> Vec<String> {
        let from_components = self.get_path_components();
        let to_components = to.get_path_components();

        let mut start = 0;
        while start < from_components.len() && start < to_components.len() {
            let from_component = &from_components[start];
            let to_component = &to_components[start];

            let matched = if self.is_case_sensitive() {
                from_component == to_component
            } else {
                from_component.js_to_lower_case() == to_component.js_to_lower_case()
            };

            if !matched {
                break;
            }
            start += 1;
        }

        if start == 0 {
            return to_components;
        }

        let components = &to_components[start..];
        let mut relative = Vec::new();
        while start < from_components.len() {
            relative.push("..".to_string());
            start += 1;
        }
        relative.extend(components.iter().cloned());
        relative
    }

    // common/uri/baseUri.ts:238
    pub fn get_shortened_file_name(&self, max_dir_length: i32) -> String {
        get_shortened_file_name(self.get_path(), max_dir_length)
    }
}

// common/uri/baseUri.ts:255
pub fn combine_path_elements(path_string: &str, separator: &str, paths: &[&str]) -> String {
    let mut path_string = path_string.to_string();
    for relative_path in paths {
        if relative_path.is_empty() {
            continue;
        }
        if path_string.is_empty() || get_root_length(relative_path) != 0 {
            path_string = relative_path.to_string();
        } else if path_string.ends_with(separator) {
            path_string.push_str(relative_path);
        } else {
            path_string.push_str(separator);
            path_string.push_str(relative_path);
        }
    }

    path_string
}
