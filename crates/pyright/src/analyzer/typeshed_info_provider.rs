//! Port of `packages/pyright-internal/src/analyzer/typeshedInfoProvider.ts`.

use jsrt::cell::RefCell;

use jsrt::{FxHashMap, JsMap, JsSet, JsStr, P};

use crate::analyzer::import_logger::ImportLogger;
use crate::analyzer::import_resolver_types::{
    ImportResolverFileSystem, SupportedVersionInfo, TypeshedInfoProvider,
    TypeshedThirdPartyPackageMapResult,
};
use crate::analyzer::python_path_utils;
use crate::common::file_system::{FsResult, ReadOnlyFileSystem};
use crate::common::path_utils::strip_file_extension;
use crate::common::python_version::{PYTHON_VERSION3_0, PythonVersion};
use crate::common::uri::Uri;

// analyzer/typeshedInfoProvider.ts:20
pub fn create_default_typeshed_info_provider(
    file_system: &'static ImportResolverFileSystem,
) -> TypeshedInfoProvider {
    DefaultTypeshedInfoProvider::new(file_system)
}

// analyzer/typeshedInfoProvider.ts:24
#[derive(jsrt::Trace)]
pub struct DefaultTypeshedInfoProvider {
    _typeshed_root_cache: RefCell<FxHashMap<String, Option<Uri>>>,
    _typeshed_subdirectory_cache: RefCell<FxHashMap<String, Option<Uri>>>,
    _third_party_package_map_cache: RefCell<FxHashMap<String, TypeshedThirdPartyPackageMapResult>>,
    _stdlib_version_info_cache: RefCell<FxHashMap<String, P<JsMap<String, SupportedVersionInfo>>>>,

    _file_system: &'static ImportResolverFileSystem,
}

impl DefaultTypeshedInfoProvider {
    // analyzer/typeshedInfoProvider.ts:30
    fn new(file_system: &'static ImportResolverFileSystem) -> DefaultTypeshedInfoProvider {
        DefaultTypeshedInfoProvider {
            _typeshed_root_cache: RefCell::new(FxHashMap::default()),
            _typeshed_subdirectory_cache: RefCell::new(FxHashMap::default()),
            _third_party_package_map_cache: RefCell::new(FxHashMap::default()),
            _stdlib_version_info_cache: RefCell::new(FxHashMap::default()),
            _file_system: file_system,
        }
    }

    // analyzer/typeshedInfoProvider.ts:34
    pub fn get_typeshed_root(
        &self,
        custom_typeshed_path: Option<Uri>,
        _import_logger: Option<&ImportLogger>,
    ) -> Option<Uri> {
        let key = custom_typeshed_path.map_or("", |p| p.key()).to_string();
        // An `undefined` entry counts as a miss.
        let cached = self
            ._typeshed_root_cache
            .borrow()
            .get(&key)
            .copied()
            .flatten();
        if let Some(cached) = cached {
            return Some(cached);
        }

        let root = self._compute_typeshed_root(custom_typeshed_path);
        self._typeshed_root_cache.borrow_mut().insert(key, root);
        root
    }

    // analyzer/typeshedInfoProvider.ts:46
    pub fn get_typeshed_subdirectory(
        &self,
        is_std_lib: bool,
        custom_typeshed_path: Option<Uri>,
        import_logger: Option<&ImportLogger>,
    ) -> Option<Uri> {
        let key = format!(
            "{}:{}",
            if is_std_lib { "stdlib" } else { "thirdParty" },
            custom_typeshed_path.map_or("", |p| p.key())
        );
        // An `undefined` entry counts as a miss.
        let cached = self
            ._typeshed_subdirectory_cache
            .borrow()
            .get(&key)
            .copied()
            .flatten();
        if let Some(cached) = cached {
            return Some(cached);
        }

        let typeshed_root = self.get_typeshed_root(custom_typeshed_path, import_logger);
        let Some(typeshed_root) = typeshed_root else {
            self._typeshed_subdirectory_cache
                .borrow_mut()
                .insert(key, None);
            return None;
        };

        let subdir = python_path_utils::get_typeshed_subdirectory(typeshed_root, is_std_lib);
        if !self._file_system.dir_exists(subdir) {
            self._typeshed_subdirectory_cache
                .borrow_mut()
                .insert(key, None);
            return None;
        }

        self._typeshed_subdirectory_cache
            .borrow_mut()
            .insert(key, Some(subdir));
        Some(subdir)
    }

    // analyzer/typeshedInfoProvider.ts:73
    pub fn get_third_party_package_map(
        &self,
        custom_typeshed_path: Option<Uri>,
        import_logger: Option<&ImportLogger>,
    ) -> TypeshedThirdPartyPackageMapResult {
        let key = custom_typeshed_path.map_or("", |p| p.key()).to_string();
        let cached = self
            ._third_party_package_map_cache
            .borrow()
            .get(&key)
            .copied();
        if let Some(cached) = cached {
            return cached;
        }

        let third_party_dir = self.get_typeshed_subdirectory(
            /* isStdLib */ false,
            custom_typeshed_path,
            import_logger,
        );
        let mut typeshed_third_party_package_paths: JsMap<String, Vec<Uri>> = JsMap::default();

        if let Some(third_party_dir) = third_party_dir {
            // `readdirEntriesSync` is cached by ImportResolverFileSystem, so repeated calls across
            // ImportResolvers will share the same cached directory enumerations.
            for outer_entry in self._file_system.readdir_entries_sync(third_party_dir) {
                if !outer_entry.is_directory {
                    continue;
                }

                let inner_dir_path = third_party_dir.combine_paths(&[&outer_entry.name]);

                for inner_entry in self._file_system.readdir_entries_sync(inner_dir_path) {
                    if inner_entry.name == "@python2" {
                        continue;
                    }

                    if inner_entry.is_directory {
                        let path_list =
                            typeshed_third_party_package_paths.get_mut(&inner_entry.name);
                        if let Some(path_list) = path_list {
                            path_list.push(inner_dir_path);
                        } else {
                            typeshed_third_party_package_paths
                                .insert(inner_entry.name.clone(), vec![inner_dir_path]);
                        }
                    } else if inner_entry.is_file {
                        if inner_entry.name.ends_with(".pyi") {
                            let stripped_file_name = strip_file_extension(&inner_entry.name, false);
                            let path_list =
                                typeshed_third_party_package_paths.get_mut(&stripped_file_name);
                            if let Some(path_list) = path_list {
                                path_list.push(inner_dir_path);
                            } else {
                                typeshed_third_party_package_paths
                                    .insert(stripped_file_name, vec![inner_dir_path]);
                            }
                        }
                    }
                }
            }
        }

        let flatten_paths: Vec<Uri> = typeshed_third_party_package_paths
            .values()
            .flatten()
            .copied()
            .collect();
        // `Array.prototype.sort` compares the elements' `toString()`.
        let mut package_roots: Vec<Uri> = flatten_paths
            .into_iter()
            .collect::<JsSet<Uri>>()
            .into_iter()
            .collect();
        package_roots.sort_by(|a, b| {
            a.to_uri_string()
                .as_str()
                .js_cmp(b.to_uri_string().as_str())
        });
        let result: TypeshedThirdPartyPackageMapResult =
            P::new((typeshed_third_party_package_paths, package_roots));

        self._third_party_package_map_cache
            .borrow_mut()
            .insert(key, result);
        result
    }

    // analyzer/typeshedInfoProvider.ts:133
    pub fn get_std_lib_module_version_info(
        &self,
        custom_typeshed_path: Option<Uri>,
        import_logger: Option<&ImportLogger>,
    ) -> P<JsMap<String, SupportedVersionInfo>> {
        let key = custom_typeshed_path.map_or("", |p| p.key()).to_string();
        let cached = self._stdlib_version_info_cache.borrow().get(&key).copied();
        if let Some(cached) = cached {
            return cached;
        }

        let mut version_range_map: JsMap<String, SupportedVersionInfo> = JsMap::default();

        // Read the VERSIONS file from typeshed.
        let typeshed_std_lib_path = self.get_typeshed_subdirectory(
            /* isStdLib */ true,
            custom_typeshed_path,
            import_logger,
        );
        if let Some(typeshed_std_lib_path) = typeshed_std_lib_path {
            let versions_file_path = typeshed_std_lib_path.combine_paths(&["VERSIONS"]);
            let read_result = (|| -> FsResult<()> {
                let file_stats = self._file_system.stat_sync(versions_file_path)?;
                if file_stats.size > 0 && file_stats.size < 256 * 1024 {
                    let file_contents =
                        self._file_system.read_file_text_sync(versions_file_path)?;
                    // `split(/\r?\n/)`
                    let mut lines: Vec<&str> = file_contents.split('\n').collect();
                    let last = lines.len() - 1;
                    for line in &mut lines[..last] {
                        *line = line.strip_suffix('\r').unwrap_or(line);
                    }
                    lines.into_iter().for_each(|line| {
                        let comment_split: Vec<&str> = line.split('#').collect();

                        // Platform-specific information can be specified after a semicolon.
                        let semicolon_split: Vec<&str> =
                            comment_split[0].split(';').map(|s| s.js_trim()).collect();

                        // Version information is found after a colon.
                        let colon_split: Vec<&str> = semicolon_split[0].split(':').collect();
                        if colon_split.len() != 2 {
                            return;
                        }

                        let version_split: Vec<&str> = colon_split[1].split('-').collect();
                        if version_split.len() > 2 {
                            return;
                        }

                        let module_name = colon_split[0].js_trim();
                        if module_name.is_empty() {
                            return;
                        }

                        let mut min_version_string = version_split[0].js_trim();
                        if min_version_string.ends_with('+') {
                            // If the version ends in "+", strip it off.
                            min_version_string =
                                &min_version_string[..min_version_string.len() - 1];
                        }

                        let mut min_version = PythonVersion::from_string(min_version_string);
                        if min_version.is_none() {
                            min_version = Some(PYTHON_VERSION3_0);
                        }

                        let mut max_version: Option<PythonVersion> = None;
                        if version_split.len() > 1 {
                            max_version = PythonVersion::from_string(version_split[1].js_trim());
                        }

                        // A semicolon can be followed by a semicolon-delimited list of other
                        // exclusions. The "platform" exclusion is a comma delimited list platforms
                        // that are supported or not supported.
                        let mut supported_platforms: Option<Vec<String>> = None;
                        let mut unsupported_platforms: Option<Vec<String>> = None;
                        let platforms_header = "platforms=";
                        let platform_exclusions = semicolon_split[1..]
                            .iter()
                            .find(|s| s.starts_with(platforms_header));

                        if let Some(platform_exclusions) = platform_exclusions {
                            let platform_exclusions =
                                &platform_exclusions.js_trim()[platforms_header.len()..];
                            let comma_split = platform_exclusions.split(',');
                            for platform in comma_split {
                                let mut platform = platform.js_trim();
                                let mut is_unsupported = false;

                                // Remove the '!' from the start if it's an exclusion.
                                if platform.starts_with('!') {
                                    is_unsupported = true;
                                    platform = &platform[1..];
                                }

                                if is_unsupported {
                                    unsupported_platforms
                                        .get_or_insert_with(Vec::new)
                                        .push(platform.to_string());
                                } else {
                                    supported_platforms
                                        .get_or_insert_with(Vec::new)
                                        .push(platform.to_string());
                                }
                            }
                        }

                        version_range_map.insert(
                            module_name.to_string(),
                            SupportedVersionInfo {
                                min: min_version.unwrap_or(PYTHON_VERSION3_0),
                                max: max_version,
                                supported_platforms,
                                unsupported_platforms,
                            },
                        );
                    });
                } else if let Some(logger) = import_logger {
                    logger.log("Typeshed stdlib VERSIONS file is unexpectedly large");
                }
                Ok(())
            })();
            if let Err(e) = read_result {
                // pyright logs `JSON.stringify(e)`: the error's own enumerable
                // properties, which `FsError` does not keep.
                if let Some(logger) = import_logger {
                    logger.log(&format!(
                        "Could not read typeshed stdlib VERSIONS file: '{}'",
                        e.0
                    ));
                }
            }
        }

        let version_range_map = P::new(version_range_map);
        self._stdlib_version_info_cache
            .borrow_mut()
            .insert(key, version_range_map);
        version_range_map
    }

    // analyzer/typeshedInfoProvider.ts:241
    fn _compute_typeshed_root(&self, custom_typeshed_path: Option<Uri>) -> Option<Uri> {
        // Did the user specify a typeshed path? If not, use the fallback.
        if let Some(custom_typeshed_path) = custom_typeshed_path {
            if self._file_system.dir_exists(custom_typeshed_path) {
                return Some(custom_typeshed_path);
            }
        }

        let fallback = python_path_utils::get_type_shed_fallback_path(self._file_system)
            .unwrap_or(Uri::empty());
        if fallback.is_empty() {
            None
        } else {
            Some(fallback)
        }
    }
}

#[cfg(test)]
mod tests {
    //! Port of the `TypeshedInfoProvider (default)` cases in `tests/importResolverSupport.test.ts`.

    use super::*;
    use crate::analyzer::import_resolver_file_system::create_import_resolver_file_system;
    use crate::common::path_consts::TYPESHED_FALLBACK;
    use crate::common::python_version::{
        PYTHON_VERSION3_4, PYTHON_VERSION3_6, PYTHON_VERSION3_8, PYTHON_VERSION3_10,
    };
    use crate::tests::harness::vfs::filesystem::{TestFileSystem, file};

    struct Layout {
        fs: &'static TestFileSystem,
        cache: &'static ImportResolverFileSystem,
        provider: TypeshedInfoProvider,
        typeshed_root: String,
    }

    fn create_fs_with_typeshed_layout() -> Layout {
        let fs = TestFileSystem::leak();

        let typeshed_root = format!("/{TYPESHED_FALLBACK}");
        fs.mkdirp_sync(&typeshed_root);
        fs.mkdirp_sync(&format!("{typeshed_root}/stdlib"));
        fs.mkdirp_sync(&format!("{typeshed_root}/stubs"));

        let cache: &'static ImportResolverFileSystem =
            P::new(create_import_resolver_file_system(fs)).get();
        let provider = create_default_typeshed_info_provider(cache);

        Layout {
            fs,
            cache,
            provider,
            typeshed_root,
        }
    }

    fn sorted_paths(uris: &[Uri]) -> Vec<String> {
        let mut paths: Vec<String> = uris.iter().map(|u| u.get_file_path().to_string()).collect();
        paths.sort();
        paths
    }

    #[test]
    fn get_typeshed_root_prefers_custom_path_when_present_otherwise_falls_back() {
        let Layout {
            fs,
            provider,
            typeshed_root,
            ..
        } = create_fs_with_typeshed_layout();

        fs.mkdirp_sync("/customTypeshed");

        let custom = file("/customTypeshed");
        let resolved_custom = provider.get_typeshed_root(Some(custom), None);
        assert_eq!(
            resolved_custom.map(|u| u.get_file_path()),
            Some("/customTypeshed")
        );

        let resolved_fallback = provider.get_typeshed_root(None, None);
        assert_eq!(
            resolved_fallback.map(|u| u.get_file_path()),
            Some(typeshed_root.as_str())
        );
    }

    #[test]
    fn get_typeshed_root_ignores_a_custom_typeshed_path_that_is_not_a_directory() {
        let Layout {
            fs,
            provider,
            typeshed_root,
            ..
        } = create_fs_with_typeshed_layout();

        fs.write_file_sync("/customTypeshed", "not a directory");

        let resolved = provider.get_typeshed_root(Some(file("/customTypeshed")), None);
        assert_eq!(
            resolved.map(|u| u.get_file_path()),
            Some(typeshed_root.as_str())
        );
    }

    #[test]
    fn get_typeshed_subdirectory_returns_undefined_when_the_expected_subdirectory_does_not_exist() {
        let fs = TestFileSystem::leak();

        let typeshed_root = format!("/{TYPESHED_FALLBACK}");
        fs.mkdirp_sync(&typeshed_root);
        fs.mkdirp_sync(&format!("{typeshed_root}/stdlib"));
        // Intentionally omit `${typeshedRoot}/stubs`.

        let cache: &'static ImportResolverFileSystem =
            P::new(create_import_resolver_file_system(fs)).get();
        let provider = create_default_typeshed_info_provider(cache);

        assert_eq!(
            provider.get_typeshed_subdirectory(/* isStdLib */ false, None, None),
            None
        );
    }

    #[test]
    fn get_third_party_package_map_builds_expected_map_and_is_memoized() {
        let Layout {
            fs,
            cache,
            provider,
            typeshed_root,
        } = create_fs_with_typeshed_layout();

        // Build a minimal stubs layout:
        //   stubs/a/foo/
        //   stubs/a/bar.pyi
        //   stubs/a/@python2/   (ignored)
        //   stubs/b/foo/
        fs.mkdirp_sync(&format!("{typeshed_root}/stubs/a/foo"));
        fs.mkdirp_sync(&format!("{typeshed_root}/stubs/a/@python2"));
        fs.write_file_sync(&format!("{typeshed_root}/stubs/a/bar.pyi"), "");
        fs.write_file_sync(&format!("{typeshed_root}/stubs/a/readme.txt"), "");
        fs.mkdirp_sync(&format!("{typeshed_root}/stubs/b/foo"));
        fs.write_file_sync(&format!("{typeshed_root}/stubs/outerFile.txt"), "");

        let result = provider.get_third_party_package_map(None, None);
        let (package_map, roots) = result.get();
        let a = format!("{typeshed_root}/stubs/a");
        let b = format!("{typeshed_root}/stubs/b");
        assert_eq!(sorted_paths(roots), [a.clone(), b.clone()]);

        assert_eq!(sorted_paths(&package_map["foo"]), [a.clone(), b.clone()]);
        assert_eq!(sorted_paths(&package_map["bar"]), [a]);

        assert!(!package_map.contains_key("@python2"));
        assert!(!package_map.contains_key("readme"));

        let calls_after_first = fs.call_count("readdirEntriesSync");

        // Clear the ImportResolverFileSystem cache to ensure TypeshedInfoProvider memoization is effective.
        cache.invalidate_cache();

        assert_eq!(provider.get_third_party_package_map(None, None), result);

        // Should not have re-enumerated directories.
        assert_eq!(fs.call_count("readdirEntriesSync"), calls_after_first);
    }

    #[test]
    fn get_std_lib_module_version_info_parses_versions_and_is_memoized() {
        let Layout {
            fs,
            cache,
            provider,
            typeshed_root,
        } = create_fs_with_typeshed_layout();

        fs.write_file_sync(
            &format!("{typeshed_root}/stdlib/VERSIONS"),
            &[
                "asyncio: 3.4-",
                "distutils: 3.0-3.10 ; platforms=win32, !linux",
                "bar: -3.8",
                "email: 3.6+  # plus suffix is allowed",
                "this is not valid",
                ": 3.7-  # missing module name",
            ]
            .join("\n"),
        );

        let version_info = provider.get_std_lib_module_version_info(None, None);

        let asyncio = &version_info["asyncio"];
        assert!(PythonVersion::is_equal_to(&asyncio.min, &PYTHON_VERSION3_4));
        assert_eq!(asyncio.max, None);

        let distutils = &version_info["distutils"];
        assert!(PythonVersion::is_equal_to(
            &distutils.min,
            &PYTHON_VERSION3_0
        ));
        assert!(PythonVersion::is_equal_to(
            distutils.max.as_ref().unwrap(),
            &PYTHON_VERSION3_10
        ));
        assert_eq!(
            distutils.supported_platforms.as_deref(),
            Some(&["win32".to_string()][..])
        );
        assert_eq!(
            distutils.unsupported_platforms.as_deref(),
            Some(&["linux".to_string()][..])
        );

        let bar = &version_info["bar"];
        // Empty min version defaults to 3.0.
        assert!(PythonVersion::is_equal_to(&bar.min, &PYTHON_VERSION3_0));
        assert!(PythonVersion::is_equal_to(
            bar.max.as_ref().unwrap(),
            &PYTHON_VERSION3_8
        ));

        let email = &version_info["email"];
        assert!(PythonVersion::is_equal_to(&email.min, &PYTHON_VERSION3_6));

        let stat_calls_after_first = fs.call_count("statSync");
        let read_calls_after_first = fs.call_count("readFileSync");

        // Clear the ImportResolverFileSystem cache to ensure TypeshedInfoProvider memoization is effective.
        cache.invalidate_cache();

        assert_eq!(
            provider.get_std_lib_module_version_info(None, None),
            version_info
        );

        assert_eq!(fs.call_count("statSync"), stat_calls_after_first);
        assert_eq!(fs.call_count("readFileSync"), read_calls_after_first);
    }

    #[test]
    fn get_std_lib_module_version_info_memoizes_an_empty_result_when_versions_is_missing() {
        let Layout {
            fs,
            provider,
            typeshed_root,
            ..
        } = create_fs_with_typeshed_layout();

        // No VERSIONS file.
        let version_info1 = provider.get_std_lib_module_version_info(None, None);
        assert_eq!(version_info1.len(), 0);

        // Create it later; memoization should keep returning the originally computed value.
        fs.write_file_sync(&format!("{typeshed_root}/stdlib/VERSIONS"), "asyncio: 3.4-");
        let version_info2 = provider.get_std_lib_module_version_info(None, None);
        assert_eq!(version_info2, version_info1);
        assert_eq!(version_info2.len(), 0);
    }

    #[test]
    fn get_std_lib_module_version_info_logs_and_skips_reading_when_versions_is_too_large() {
        let Layout {
            fs,
            provider,
            typeshed_root,
            ..
        } = create_fs_with_typeshed_layout();

        fs.write_file_sync(
            &format!("{typeshed_root}/stdlib/VERSIONS"),
            &"a".repeat(256 * 1024),
        );

        let import_logger = ImportLogger::new();

        let version_info = provider.get_std_lib_module_version_info(None, Some(&import_logger));
        assert_eq!(version_info.len(), 0);
        assert_eq!(fs.call_count("readFileSync"), 0);

        assert!(
            import_logger
                .get_logs()
                .iter()
                .any(|l| l.contains("unexpectedly large"))
        );
    }

    #[test]
    fn get_std_lib_module_version_info_logs_and_returns_empty_when_reading_versions_throws() {
        let Layout {
            fs,
            provider,
            typeshed_root,
            ..
        } = create_fs_with_typeshed_layout();

        fs.write_file_sync(&format!("{typeshed_root}/stdlib/VERSIONS"), "asyncio: 3.4-");

        let import_logger = ImportLogger::new();
        fs.fail_read.set(true);

        let version_info = provider.get_std_lib_module_version_info(None, Some(&import_logger));
        assert_eq!(version_info.len(), 0);
        assert!(
            import_logger
                .get_logs()
                .iter()
                .any(|l| l.contains("Could not read typeshed stdlib VERSIONS file"))
        );
    }

    #[test]
    fn versions_lines_split_on_crlf_and_keep_a_lone_cr() {
        let Layout {
            fs,
            provider,
            typeshed_root,
            ..
        } = create_fs_with_typeshed_layout();

        fs.write_file_sync(
            &format!("{typeshed_root}/stdlib/VERSIONS"),
            "a: 3.4-\r\nx\ry: 3.5-\nc: 3.6-3.7\r",
        );

        let version_info = provider.get_std_lib_module_version_info(None, None);
        let names: Vec<&str> = version_info.keys().map(String::as_str).collect();
        assert_eq!(names, ["a", "x\ry", "c"]);
        assert_eq!(
            version_info["c"].max,
            Some(PythonVersion::create(3, 7, None, None, None))
        );
    }

    #[test]
    fn third_party_package_roots_sort_by_uri_string() {
        let Layout {
            fs,
            provider,
            typeshed_root,
            ..
        } = create_fs_with_typeshed_layout();

        // `=` becomes "%3D" in the Uri string, which sorts before "-"; in the
        // plain path `-` sorts before `=`.
        for dir in ["b", "a=b", "a-b"] {
            fs.mkdirp_sync(&format!("{typeshed_root}/stubs/{dir}/pkg"));
        }

        let result = provider.get_third_party_package_map(None, None);
        let roots: Vec<&str> = result.get().1.iter().map(|u| u.file_name()).collect();
        assert_eq!(roots, ["a=b", "a-b", "b"]);
    }
}
