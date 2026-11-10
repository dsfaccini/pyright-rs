//! Port of `packages/pyright-internal/src/analyzer/pythonPathUtils.ts`.

use std::cmp::Ordering;

use jsrt::JsStr;

use crate::analyzer::import_logger::ImportLogger;
use crate::common::config_options::ConfigOptions;
use crate::common::core::{Comparison, compare_comparable_values_str};
use crate::common::file_system::{FileSystem, ReadOnlyFileSystem};
use crate::common::host::Host;
use crate::common::path_consts;
use crate::common::python_version::PythonVersion;
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::{get_file_system_entries, is_directory, try_stat};

// analyzer/pythonPathUtils.ts:20
#[derive(Clone, Debug)]
pub struct PythonPathResult {
    pub paths: Vec<Uri>,
    pub prefix: Option<Uri>,
}

// analyzer/pythonPathUtils.ts:25
pub const STD_LIB_FOLDER_NAME: &str = "stdlib";
// analyzer/pythonPathUtils.ts:26
pub const THIRD_PARTY_FOLDER_NAME: &str = "stubs";

// analyzer/pythonPathUtils.ts:28
pub fn get_type_shed_fallback_path(fs: &dyn ReadOnlyFileSystem) -> Option<Uri> {
    let module_directory = fs.get_module_path();
    if module_directory.is_empty() {
        return None;
    }

    let typeshed_path = module_directory.combine_paths(&[path_consts::TYPESHED_FALLBACK]);
    if fs.exists_sync(typeshed_path) {
        return Some(fs.real_case_path(typeshed_path));
    }

    // In the debug version of Pyright, the code is one level
    // deeper, so we need to look one level up for the typeshed fallback.
    let debug_typeshed_path = module_directory
        .get_directory()
        .combine_paths(&[path_consts::TYPESHED_FALLBACK]);
    if fs.exists_sync(debug_typeshed_path) {
        return Some(fs.real_case_path(debug_typeshed_path));
    }

    None
}

// analyzer/pythonPathUtils.ts:49
pub fn get_typeshed_subdirectory(typeshed_path: Uri, is_std_lib: bool) -> Uri {
    typeshed_path.combine_paths(&[if is_std_lib {
        STD_LIB_FOLDER_NAME
    } else {
        THIRD_PARTY_FOLDER_NAME
    }])
}

// analyzer/pythonPathUtils.ts:53
pub fn find_python_search_paths(
    fs: &dyn FileSystem,
    config_options: &ConfigOptions,
    host: &dyn Host,
    import_logger: Option<&ImportLogger>,
    include_watch_paths_only: bool,
    workspace_root: Option<Uri>,
) -> Vec<Uri> {
    let log = |message: &str| {
        if let Some(logger) = import_logger {
            logger.log(message);
        }
    };
    log("Finding python search paths");

    if let (Some(venv_path_root), Some(venv_dir)) =
        (config_options.venv_path, config_options.venv.as_deref())
    {
        if !venv_dir.is_empty() {
            let venv_path = venv_path_root.combine_paths(&[venv_dir]);

            let mut found_paths: Vec<Uri> = Vec::new();
            let mut site_packages_paths: Vec<Uri> = Vec::new();

            for lib_path in [
                path_consts::LIB,
                path_consts::LIB64,
                path_consts::LIB_ALTERNATE,
            ] {
                let site_packages_path = find_site_packages_path(
                    fs,
                    venv_path.combine_paths(&[lib_path]),
                    config_options.default_python_version,
                    import_logger,
                );
                if let Some(site_packages_path) = site_packages_path {
                    add_path_if_unique(&mut found_paths, site_packages_path);
                    site_packages_paths.push(fs.real_case_path(site_packages_path));
                }
            }

            // Now add paths from ".pth" files located in each of the site packages folders.
            for site_packages_path in &site_packages_paths {
                for path in get_paths_from_pth_files(fs, *site_packages_path) {
                    add_path_if_unique(&mut found_paths, path);
                }
            }

            if !found_paths.is_empty() {
                // If a pythonPath is configured and it belongs to this venv,
                // also include the non-site-packages paths it reports.
                if config_options.python_path.is_some() {
                    let path_result = host.get_python_search_paths(
                        config_options.python_path,
                        import_logger,
                        Some(config_options.project_root),
                    );
                    let real_venv_path = fs.real_case_path(venv_path);
                    if path_result
                        .prefix
                        .is_some_and(|p| p.equals(Some(real_venv_path)))
                    {
                        for path in &path_result.paths {
                            let real_case_path = fs.real_case_path(*path);
                            if !real_case_path.path_ends_with(path_consts::SITE_PACKAGES)
                                && !real_case_path.path_ends_with(path_consts::DIST_PACKAGES)
                            {
                                add_path_if_unique(&mut found_paths, real_case_path);
                            }
                        }
                    }
                }

                log(&format!(
                    "Found the following '{}' dirs",
                    path_consts::SITE_PACKAGES
                ));
                for path in &found_paths {
                    log(&format!("  {}", path.to_uri_string()));
                }
                return found_paths
                    .into_iter()
                    .filter(|p| is_directory(fs, *p))
                    .collect();
            }

            log(&format!(
                "Did not find any '{}' dirs. Falling back on python interpreter.",
                path_consts::SITE_PACKAGES
            ));
        }
    }

    // Fall back on the python interpreter.
    let path_result = host.get_python_search_paths(
        config_options.python_path,
        import_logger,
        Some(config_options.project_root),
    );
    if include_watch_paths_only {
        if let Some(workspace_root) = workspace_root.filter(|r| !r.is_empty()) {
            return path_result
                .paths
                .iter()
                .filter(|p| {
                    !p.starts_with(Some(workspace_root)) || p.starts_with(path_result.prefix)
                })
                .map(|p| fs.real_case_path(*p))
                .collect();
        }
    }

    path_result
        .paths
        .iter()
        .map(|p| fs.real_case_path(*p))
        .collect()
}

// analyzer/pythonPathUtils.ts:141
pub fn is_python_binary(p: &str) -> bool {
    let p = p.js_trim();
    p == "python" || p == "python3"
}

// analyzer/pythonPathUtils.ts:146
fn find_site_packages_path(
    fs: &dyn FileSystem,
    lib_path: Uri,
    python_version: Option<PythonVersion>,
    import_logger: Option<&ImportLogger>,
) -> Option<Uri> {
    let log = |message: &str| {
        if let Some(logger) = import_logger {
            logger.log(message);
        }
    };
    if fs.exists_sync(lib_path) {
        log(&format!(
            "Found path '{}'; looking for {}",
            lib_path.to_uri_string(),
            path_consts::SITE_PACKAGES
        ));
    } else {
        log(&format!("Did not find '{}'", lib_path.to_uri_string()));
        return None;
    }

    let site_packages_path = lib_path.combine_paths(&[path_consts::SITE_PACKAGES]);
    if fs.exists_sync(site_packages_path) {
        log(&format!(
            "Found path '{}'",
            site_packages_path.to_uri_string()
        ));
        return Some(site_packages_path);
    } else {
        log(&format!(
            "Did not find '{}', so looking for python subdirectory",
            site_packages_path.to_uri_string()
        ));
    }

    // We didn't find a site-packages directory directly in the lib
    // directory. Scan for a "python3.X" directory instead.
    let entries = get_file_system_entries(fs, lib_path);

    // Candidate directories start with "python3.".
    let candidate_dirs: Vec<Uri> = entries
        .directories
        .into_iter()
        .filter(|dir_name| {
            if dir_name.file_name().starts_with("python3.") {
                let dir_path = dir_name.combine_paths(&[path_consts::SITE_PACKAGES]);
                return fs.exists_sync(dir_path);
            }
            false
        })
        .collect();

    // If there is a python3.X directory (where 3.X matches the configured python
    // version), prefer that over other python directories.
    if let Some(python_version) = python_version {
        let wanted = format!(
            "python{}",
            PythonVersion::to_major_minor_string(&python_version)
        );
        if let Some(preferred_dir) = candidate_dirs
            .iter()
            .find(|dir_name| dir_name.file_name() == wanted)
        {
            let dir_path = preferred_dir.combine_paths(&[path_consts::SITE_PACKAGES]);
            log(&format!("Found path '{}'", dir_path.to_uri_string()));
            return Some(dir_path);
        }
    }

    // If there was no python version or we didn't find an exact match, use the
    // first directory that starts with "python". Most of the time, there will be
    // only one.
    if let Some(first) = candidate_dirs.first() {
        let dir_path = first.combine_paths(&[path_consts::SITE_PACKAGES]);
        log(&format!("Found path '{}'", dir_path.to_uri_string()));
        return Some(dir_path);
    }

    None
}

// analyzer/pythonPathUtils.ts:205
pub fn read_pth_search_paths(pth_file: Uri, fs: &dyn FileSystem) -> Vec<Uri> {
    let mut search_paths: Vec<Uri> = Vec::new();

    if fs.exists_sync(pth_file) {
        // `readFileSync` throws on an unreadable file; pyright does not catch it.
        let data = fs.read_file_text_sync(pth_file).unwrap_or_default();
        // `data.split(/\r?\n/)`
        let lines: Vec<&str> = data
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .collect();
        for line in lines {
            let trimmed_line = line.js_trim();
            if !trimmed_line.is_empty()
                && !trimmed_line.starts_with('#')
                && !is_import_line(trimmed_line)
            {
                let pth_path = pth_file.get_directory().combine_paths(&[trimmed_line]);
                if fs.exists_sync(pth_path) && is_directory(fs, pth_path) {
                    search_paths.push(fs.real_case_path(pth_path));
                }
            }
        }
    }
    search_paths
}

/// `/^import\s/`: JS `\s` includes Unicode whitespace.
fn is_import_line(line: &str) -> bool {
    line.strip_prefix("import")
        .and_then(|rest| rest.chars().next())
        .is_some_and(crate::common::core::is_js_whitespace_char)
}

// analyzer/pythonPathUtils.ts:225
pub fn get_paths_from_pth_files(fs: &dyn FileSystem, parent_dir: Uri) -> Vec<Uri> {
    let mut search_paths: Vec<Uri> = Vec::new();

    // Get a list of all *.pth files within the specified directory.
    // `readdirEntriesSync` throws on a missing directory; callers pass existing ones.
    let mut pth_files: Vec<_> = fs
        .readdir_entries_sync(parent_dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| (entry.is_file || entry.is_symbolic_link) && entry.name.ends_with(".pth"))
        .collect();
    pth_files.sort_by(
        |a, b| match compare_comparable_values_str(Some(&a.name), Some(&b.name)) {
            Comparison::LessThan => Ordering::Less,
            Comparison::EqualTo => Ordering::Equal,
            Comparison::GreaterThan => Ordering::Greater,
        },
    );

    for pth_file in pth_files {
        let file_path = fs.real_case_path(parent_dir.combine_paths(&[&pth_file.name]));
        let file_stats = try_stat(fs, file_path);

        // Skip all files that are much larger than expected.
        if let Some(stats) = file_stats {
            if stats.is_file && stats.size > 0 && stats.size < 64 * 1024 {
                search_paths.extend(read_pth_search_paths(file_path, fs));
            }
        }
    }

    search_paths
}

// analyzer/pythonPathUtils.ts:247
pub fn add_path_if_unique(path_list: &mut Vec<Uri>, path_to_add: Uri) -> bool {
    if !path_list.iter().any(|path| path.key() == path_to_add.key()) {
        path_list.push(path_to_add);
        return true;
    }

    false
}
