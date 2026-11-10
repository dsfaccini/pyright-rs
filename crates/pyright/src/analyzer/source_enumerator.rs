//! Port of `packages/pyright-internal/src/analyzer/sourceEnumerator.ts`.

use std::time::Instant;

use jsrt::{JsMap, JsSet};
use regex::Regex;

use crate::common::console::ConsoleInterface;
use crate::common::file_system::FileSystem;
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::{FileSpec, get_file_system_entries_with_symlinked_directories, try_realpath, try_stat};

// analyzer/sourceEnumerator.ts:15
pub struct SourceEnumerateResult {
    pub matches: JsMap<Uri, Uri>,
    pub auto_excluded_dirs: Vec<Uri>,
    pub is_complete: bool,
}

// analyzer/sourceEnumerator.ts:21
const ENV_MARKERS: [&[&str]; 4] = [&["bin", "activate"], &["Scripts", "activate"], &["pyvenv.cfg"], &["conda-meta"]];

// analyzer/sourceEnumerator.ts:26
const LONG_OPERATION_LIMIT_IN_MS: f64 = 10000.0;

// analyzer/sourceEnumerator.ts:27
const N_FILES_TO_SUGGEST_SUBFOLDER: u32 = 50;

// analyzer/sourceEnumerator.ts:34
const PYRIGHT_CONFIG_FILE_NAME: &str = "pyrightconfig.json";
// analyzer/sourceEnumerator.ts:35
const PYPROJECT_TOML_FILE_NAME: &str = "pyproject.toml";

// analyzer/sourceEnumerator.ts:37
struct DirToExplore {
    uri: Uri,
    include_reg_exp: Regex,
    has_directory_wildcard: bool,
}

// analyzer/sourceEnumerator.ts:43
pub struct SourceEnumerator {
    elapsed_time_in_ms: f64,
    includes_to_explore: Vec<FileSpec>,
    dirs_to_explore: Vec<DirToExplore>,
    matches: JsMap<Uri, Uri>,
    auto_exclude_dirs: Vec<Uri>,
    is_complete: bool,
    num_files_visited: u32,
    logged_long_operation_error: bool,
    seen_dirs: JsSet<String>,
    include_roots: Vec<Uri>,
    symlinked_directory_roots: JsMap<String, Uri>,
    discovered_config_files: JsMap<String, Uri>,
    excludes: Vec<FileSpec>,
    auto_exclude_venv: bool,
    fs: &'static dyn FileSystem,
    console: &'static dyn ConsoleInterface,
}

impl SourceEnumerator {
    // analyzer/sourceEnumerator.ts:69
    pub fn new(
        include: &[FileSpec],
        excludes: Vec<FileSpec>,
        auto_exclude_venv: bool,
        fs: &'static dyn FileSystem,
        console: &'static dyn ConsoleInterface,
    ) -> SourceEnumerator {
        let mut includes_to_explore = include.to_vec();
        includes_to_explore.reverse();
        let include_roots =
            include.iter().map(|spec| try_realpath(fs, spec.wildcard_root).unwrap_or(spec.wildcard_root)).collect();
        console.log("Searching for source files");
        SourceEnumerator {
            elapsed_time_in_ms: 0.0,
            includes_to_explore,
            dirs_to_explore: Vec::new(),
            matches: JsMap::default(),
            auto_exclude_dirs: Vec::new(),
            is_complete: false,
            num_files_visited: 0,
            logged_long_operation_error: false,
            seen_dirs: JsSet::default(),
            include_roots,
            symlinked_directory_roots: JsMap::default(),
            discovered_config_files: JsMap::default(),
            excludes,
            auto_exclude_venv,
            fs,
            console,
        }
    }

    // analyzer/sourceEnumerator.ts:94
    pub fn get_symlinked_directory_roots(&self) -> Vec<Uri> {
        self.symlinked_directory_roots.values().copied().collect()
    }

    // analyzer/sourceEnumerator.ts:102
    pub fn get_discovered_config_files(&self) -> Vec<Uri> {
        self.discovered_config_files.values().copied().collect()
    }

    // analyzer/sourceEnumerator.ts:108
    pub fn enumerate(&mut self, time_limit_in_ms: f64) -> SourceEnumerateResult {
        let start_time = Instant::now();

        while !self.is_complete {
            if self.do_next() && !self.is_complete {
                self.finish();
            }

            let elapsed_time = start_time.elapsed().as_millis() as f64;
            if time_limit_in_ms > 0.0 && elapsed_time > time_limit_in_ms {
                break;
            }
        }

        self.elapsed_time_in_ms += start_time.elapsed().as_millis() as f64;

        if !self.logged_long_operation_error && self.is_slow_enumeration() {
            self.console.error(&format!(
                "Enumeration of workspace source files is taking longer than {} seconds.\n\
                 This may be because:\n\
                 * You have opened your home directory or entire hard drive as a workspace\n\
                 * Your workspace contains a very large number of directories and files\n\
                 * Your workspace contains a symlink to a directory with many files\n\
                 * Your workspace is remote, and file enumeration is slow\n\
                 To reduce this time, open a workspace directory with fewer files \
                 or add a pyrightconfig.json configuration file with an \"exclude\" section to exclude \
                 subdirectories from your workspace. For more details, refer to \
                 https://github.com/microsoft/pyright/blob/main/docs/configuration.md.",
                jsrt::number::number_to_string(LONG_OPERATION_LIMIT_IN_MS * 0.001)
            ));
            self.logged_long_operation_error = true;
        }

        SourceEnumerateResult {
            matches: self.matches.clone(),
            auto_excluded_dirs: self.auto_exclude_dirs.clone(),
            is_complete: self.is_complete,
        }
    }

    // analyzer/sourceEnumerator.ts:156
    fn is_slow_enumeration(&self) -> bool {
        self.elapsed_time_in_ms >= LONG_OPERATION_LIMIT_IN_MS && self.num_files_visited >= N_FILES_TO_SUGGEST_SUBFOLDER
    }

    // analyzer/sourceEnumerator.ts:160
    fn record_symlinked_directory_root(&mut self, root: Uri) {
        for existing_root in self.symlinked_directory_roots.values() {
            if root.is_child(*existing_root) {
                return;
            }
        }

        let to_remove: Vec<String> = self
            .symlinked_directory_roots
            .iter()
            .filter(|(_, existing_root)| existing_root.is_child(root))
            .map(|(key, _)| key.clone())
            .collect();
        for key in to_remove {
            self.symlinked_directory_roots.shift_remove(&key);
        }

        self.symlinked_directory_roots.insert(root.key().to_string(), root);
    }

    // analyzer/sourceEnumerator.ts:177
    fn do_next(&mut self) -> bool {
        if let Some(dir_to_explore) = self.dirs_to_explore.pop() {
            self.explore_dir(dir_to_explore);
            return false;
        }

        if let Some(include_to_explore) = self.includes_to_explore.pop() {
            self.explore_include(include_to_explore);
            return false;
        }

        true
    }

    // analyzer/sourceEnumerator.ts:193
    fn explore_dir(&mut self, dir: DirToExplore) {
        let Some(real_dir_path) = try_realpath(self.fs, dir.uri) else {
            self.console.warn(&format!("Skipping broken link \"{}\"", dir.uri.to_uri_string()));
            return;
        };

        if real_dir_path.key() != dir.uri.key() {
            self.record_symlinked_directory_root(dir.uri);
        }

        if self.seen_dirs.contains(real_dir_path.key()) {
            self.console.info(&format!(
                "Skipping recursive symlink \"{}\" -> \"{}\"",
                dir.uri.to_uri_string(),
                real_dir_path.to_uri_string()
            ));
            return;
        }
        self.seen_dirs.insert(real_dir_path.key().to_string());

        if !self.include_roots.is_empty() && !self.include_roots.iter().any(|root| real_dir_path.starts_with(Some(*root))) {
            return;
        }

        if self.auto_exclude_venv && ENV_MARKERS.iter().any(|f| self.fs.exists_sync(dir.uri.resolve_paths(f))) {
            self.auto_exclude_dirs.push(dir.uri);
            self.console.info(&format!("Auto-excluding {}", dir.uri.to_user_visible_string()));
            return;
        }

        let entries = get_file_system_entries_with_symlinked_directories(self.fs, dir.uri);

        for symlinked_dir in entries.symlinked_directories {
            self.record_symlinked_directory_root(symlinked_dir);
        }

        for file in entries.files {
            if FileSpec::match_include_file_spec(&dir.include_reg_exp, &self.excludes, file, true) {
                self.num_files_visited += 1;
                self.matches.insert(file, file);
            }

            let file_name = file.file_name();
            if (file_name == PYRIGHT_CONFIG_FILE_NAME || file_name == PYPROJECT_TOML_FILE_NAME)
                && !FileSpec::is_in_path(file, &self.excludes)
            {
                self.discovered_config_files.insert(file.key().to_string(), file);
            }
        }

        for sub_dir in entries.directories.iter().rev() {
            if (sub_dir.matches_regex(&dir.include_reg_exp) || dir.has_directory_wildcard)
                && !FileSpec::is_in_path(*sub_dir, &self.excludes)
            {
                self.dirs_to_explore.push(DirToExplore {
                    uri: *sub_dir,
                    include_reg_exp: dir.include_reg_exp.clone(),
                    has_directory_wildcard: dir.has_directory_wildcard,
                });
            }
        }
    }

    // analyzer/sourceEnumerator.ts:268
    fn explore_include(&mut self, include_spec: FileSpec) {
        if FileSpec::is_in_path(include_spec.wildcard_root, &self.excludes) {
            return;
        }

        self.seen_dirs.clear();

        let scheme = include_spec.wildcard_root.scheme();
        if scheme != "file" && !scheme.is_empty() {
            self.console.info(&format!("Skipping file enumeration for non-file URI scheme \"{scheme}\"."));
            return;
        }

        let stat = try_stat(self.fs, include_spec.wildcard_root);
        if stat.is_some_and(|s| s.is_file) {
            self.matches.insert(include_spec.wildcard_root, include_spec.wildcard_root);
        } else if stat.is_some_and(|s| s.is_directory) {
            self.dirs_to_explore.push(DirToExplore {
                uri: include_spec.wildcard_root,
                include_reg_exp: include_spec.reg_exp,
                has_directory_wildcard: include_spec.has_directory_wildcard,
            });
        } else {
            self.console.error(&format!(
                "File or directory \"{}\" does not exist.",
                include_spec.wildcard_root.to_user_visible_string()
            ));
        }
    }

    // analyzer/sourceEnumerator.ts:300
    fn finish(&mut self) {
        self.is_complete = true;

        let file_count = self.matches.len();
        if file_count == 0 {
            self.console.info("No source files found.");
        } else {
            self.console.info(&format!("Found {file_count} source {}", if file_count == 1 { "file" } else { "files" }));
        }
    }
}
