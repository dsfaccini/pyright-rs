//! Port of `packages/pyright-internal/src/analyzer/importResolver.ts`.
//!
//! Provides the logic for resolving imports according to the
//! runtime rules of Python.
//!
//! Not ported, because only the language server, `--createstub` or nothing
//! calls them: `getCompletionSuggestions` and the `_getCompletionSuggestions*`,
//! `_addFilteredSuggestionsAbsolute`, `_isResolvableSuggestion` and
//! `_isUniqueValidSuggestion` helpers (completions), `getSourceFilesFromStub`
//! and `getTypeshedStdLibPath` (`SourceMapper`), `isSupportedImportSourceFile`,
//! `isSupportedImportFile`, `supportedSourceFileExtensions` and
//! `supportedFileExtensions` (completions, stub generation), `tmp`
//! (`TempFile`), and `isStdlibModule`, `_buildStdlibCache`,
//! `getTypeshedThirdPartyPath` and `getNativeModuleName`, which nothing calls.

use std::cell::{Cell, RefCell};

use jsrt::{JsMap, JsSet, JsStr, P};

use crate::parser::tokenizer::Tokenizer;
use crate::analyzer::import_logger::ImportLogger;
use crate::analyzer::import_resolver_file_system::create_import_resolver_file_system;
use crate::analyzer::import_resolver_types::{ImportResolverFileSystem, TypeshedInfoProvider};
use crate::analyzer::import_result::{ImplicitImport, ImportResult, ImportType};
use crate::analyzer::import_statement_utils::get_directory_leading_dots_points_to;
use crate::analyzer::parent_directory_cache::{CacheEntry, ImportPath, ParentDirectoryCache};
use crate::analyzer::py_typed_utils::{PyTypedInfo, get_py_typed_info_for_py_typed_file};
use crate::analyzer::python_path_utils;
use crate::analyzer::typeshed_info_provider::create_default_typeshed_info_provider;
use crate::common::collection_utils::{append_array, get_map_values};
use crate::common::config_options::{ConfigOptions, ExecutionEnvironment, match_file_specs};
use crate::common::file_system::FileSystem;
use crate::common::host::Host;
use crate::common::path_consts::STUBS_SUFFIX;
use crate::common::path_utils::{get_file_extension, strip_file_extension};
use crate::common::python_version::PythonVersion;
use crate::common::service_provider::ServiceProvider;
use crate::common::string_utils::equate_strings_case_insensitive;
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::get_file_system_entries_from_dir_entries;
use crate::partial_stub_service::SupportPartialStubs;

// analyzer/importResolver.ts:36
#[derive(Clone, Debug)]
pub struct ImportedModuleDescriptor {
    pub leading_dots: i32,
    pub name_parts: Vec<String>,
    pub has_trailing_dot: Option<bool>,
    pub imported_symbols: Option<JsSet<String>>,
}

// analyzer/importResolver.ts:43
#[derive(Clone, Debug)]
pub struct ModuleNameAndType {
    pub module_name: String,
    pub import_type: ImportType,
    pub is_local_typings_file: bool,
}

// analyzer/importResolver.ts:49
/// `extends ModuleNameAndType`: the first three fields are its fields.
#[derive(Clone, Debug)]
pub struct ModuleImportInfo {
    pub module_name: String,
    pub import_type: ImportType,
    pub is_local_typings_file: bool,
    pub is_typeshed_file: bool,
    pub is_third_party_py_typed_present: bool,
}

// analyzer/importResolver.ts:54
#[derive(Clone, Debug)]
pub struct ModuleNameInfoFromPath {
    pub module_name: String,
    pub contains_invalid_characters: Option<bool>,
}

// analyzer/importResolver.ts:59
pub fn create_imported_module_descriptor(module_name: &str) -> ImportedModuleDescriptor {
    if module_name.is_empty() {
        return ImportedModuleDescriptor {
            leading_dots: 0,
            name_parts: Vec::new(),
            has_trailing_dot: None,
            imported_symbols: Some(JsSet::default()),
        };
    }

    let mut start_index = 0;
    let mut leading_dots = 0;
    while start_index < module_name.len() {
        if module_name.as_bytes()[start_index] != b'.' {
            break;
        }

        leading_dots += 1;
        start_index += 1;
    }

    ImportedModuleDescriptor {
        leading_dots,
        name_parts: module_name[start_index..]
            .split('.')
            .map(str::to_string)
            .collect(),
        has_trailing_dot: None,
        imported_symbols: Some(JsSet::default()),
    }
}

// analyzer/importResolver.ts:81
type CachedImportResults = JsMap<String, P<ImportResult>>;

// analyzer/importResolver.ts:83
const SUPPORTED_NATIVE_LIB_EXTENSIONS: [&str; 3] = [".pyd", ".so", ".dylib"];

// analyzer/importResolver.ts:92
// Should we allow partial resolution for third-party packages? Some use tricks
// to populate their package namespaces, so we might be able to partially resolve
// a multi - part import(e.g. "a.b.c") but not fully resolve it. If this is set to
// false, we will have some false positives. If it is set to true, we won't report
// errors when these partial-resolutions fail.
const ALLOW_PARTIAL_RESOLUTION_FOR_THIRD_PARTY_PACKAGES: bool = false;

// analyzer/importResolver.ts:94
/// Every method takes `&self`: the caches are cells. Cache maps keyed by
/// `execEnv.root?.key` use `Option<&str>`, because pyright stores results
/// under `undefined` for a root-less environment but looks them up under `''`.
pub struct ImportResolver {
    _cached_python_search_paths: RefCell<Option<Vec<Uri>>>,
    _cached_import_results: RefCell<JsMap<Option<&'static str>, CachedImportResults>>,
    _cached_module_name_results:
        RefCell<JsMap<Option<&'static str>, JsMap<String, ModuleImportInfo>>>,

    _file_system_cache: &'static ImportResolverFileSystem,
    _typeshed_info_provider: &'static TypeshedInfoProvider,

    cached_parent_import_results: ParentDirectoryCache,

    pub service_provider: ServiceProvider,
    _config_options: Cell<P<ConfigOptions>>,
    pub host: &'static dyn Host,
}

impl ImportResolver {
    // analyzer/importResolver.ts:105
    pub fn new(
        service_provider: ServiceProvider,
        config_options: P<ConfigOptions>,
        host: &'static dyn Host,
    ) -> ImportResolver {
        let cached_parent_import_results = ParentDirectoryCache::new();

        let file_system_cache: &'static ImportResolverFileSystem =
            P::new(create_import_resolver_file_system(service_provider.fs())).get();
        let typeshed_info_provider: &'static TypeshedInfoProvider =
            P::new(create_default_typeshed_info_provider(file_system_cache)).get();

        ImportResolver {
            _cached_python_search_paths: RefCell::new(None),
            _cached_import_results: RefCell::new(JsMap::default()),
            _cached_module_name_results: RefCell::new(JsMap::default()),
            _file_system_cache: file_system_cache,
            _typeshed_info_provider: typeshed_info_provider,
            cached_parent_import_results,
            service_provider,
            _config_options: Cell::new(config_options),
            host,
        }
    }

    // analyzer/importResolver.ts:119
    pub fn file_system(&self) -> &'static dyn FileSystem {
        self.service_provider.fs()
    }

    // analyzer/importResolver.ts:127
    pub fn partial_stubs(&self) -> &'static dyn SupportPartialStubs {
        self.service_provider.partial_stubs()
    }

    // analyzer/importResolver.ts:141
    pub fn invalidate_cache(&self) {
        *self._cached_import_results.borrow_mut() = JsMap::default();
        *self._cached_module_name_results.borrow_mut() = JsMap::default();
        self.cached_parent_import_results.reset();

        self._invalidate_file_system_cache();

        self.partial_stubs().clear_partial_stubs();
    }

    // analyzer/importResolver.ts:154
    // Resolves the import and returns the path if it exists, otherwise
    // returns undefined.
    pub fn resolve_import(
        &self,
        source_file_uri: Uri,
        exec_env: &ExecutionEnvironment,
        module_descriptor: &ImportedModuleDescriptor,
    ) -> P<ImportResult> {
        // Wrap internal call to resolveImportInternal() to prevent calling any
        // child class version of resolveImport().
        self.resolve_import_internal(source_file_uri, exec_env, module_descriptor)
    }

    // analyzer/importResolver.ts:196
    pub fn get_config_options(&self) -> P<ConfigOptions> {
        self._config_options.get()
    }

    // analyzer/importResolver.ts:200
    pub fn set_config_options(&self, config_options: P<ConfigOptions>) {
        self._config_options.set(config_options);
        self.invalidate_cache();
    }

    // analyzer/importResolver.ts:307
    // Returns the module name (of the form X.Y.Z) that needs to be imported
    // from the current context to access the module with the specified file path.
    // In a sense, it's performing the inverse of resolveImport.
    pub fn get_module_name_for_import(
        &self,
        file_uri: Uri,
        exec_env: &ExecutionEnvironment,
        allow_invalid_module_name: bool,
        detect_py_typed: bool,
    ) -> ModuleImportInfo {
        // Cache results of the reverse of resolveImport as we cache resolveImport.
        let cache_key = exec_env.root.map(|r| r.key());
        let key = format!(
            "{allow_invalid_module_name}.{detect_py_typed}.{}",
            file_uri.key()
        );
        let cached = self
            ._cached_module_name_results
            .borrow_mut()
            .entry(cache_key)
            .or_default()
            .get(&key)
            .cloned();
        if let Some(cached) = cached {
            return cached;
        }

        let new_value = self._get_module_name_for_import(
            file_uri,
            exec_env,
            allow_invalid_module_name,
            detect_py_typed,
        );
        self._cached_module_name_results
            .borrow_mut()
            .entry(cache_key)
            .or_default()
            .insert(key, new_value.clone());
        new_value
    }

    // analyzer/importResolver.ts:353
    pub fn get_import_roots(&self, exec_env: &ExecutionEnvironment, for_logging: bool) -> Vec<Uri> {
        let config_options = self._config_options.get();
        let mut roots = Vec::new();

        let std_typeshed = self._get_stdlib_typeshed_path(
            config_options.typeshed_path,
            exec_env.python_version,
            exec_env.python_platform.as_deref(),
            None,
            None,
        );
        if let Some(std_typeshed) = std_typeshed {
            roots.push(std_typeshed);
        }

        // The "default" workspace has a root-less execution environment; ignore it.
        if let Some(root) = exec_env.root {
            roots.push(root);
        }

        append_array(&mut roots, &exec_env.extra_paths);

        if let Some(stub_path) = config_options.stub_path {
            roots.push(stub_path);
        }

        if for_logging {
            // There's one path for each third party package, which blows up logging.
            // Just get the root directly and show it with `...` to indicate that this
            // is where the third party folder is in the roots.
            let third_party_root =
                self._get_third_party_typeshed_path(config_options.typeshed_path, None);
            if let Some(third_party_root) = third_party_root {
                roots.push(third_party_root.resolve_paths(&["..."]));
            }
        } else {
            let third_party_paths = self._get_third_party_typeshed_package_roots(None);
            append_array(&mut roots, &third_party_paths);
        }

        let typeshed_path_ex = self.get_typeshed_path_ex(exec_env, None);
        if let Some(typeshed_path_ex) = typeshed_path_ex {
            roots.push(typeshed_path_ex);
        }

        let python_search_paths = self.get_python_search_paths(None);
        if !python_search_paths.is_empty() {
            append_array(&mut roots, &python_search_paths);
        }

        roots
    }

    // analyzer/importResolver.ts:402
    pub fn ensure_partial_stub_packages(&self, exec_env: &ExecutionEnvironment) -> bool {
        let config_options = self._config_options.get();
        // `this.partialStubs` is never undefined: the service provider creates it.
        let ps = self.partial_stubs();

        if ps.is_partial_stub_packages_scanned(exec_env) {
            return false;
        }

        let mut paths: Vec<Uri> = Vec::new();
        let typeshed_path_ex = self.get_typeshed_path_ex(exec_env, None);

        // analyzer/importResolver.ts:427
        let add_paths = |paths: &mut Vec<Uri>, path: Option<Uri>| {
            let Some(path) = path else {
                return;
            };
            if ps.is_path_scanned(path) {
                return;
            }

            paths.push(path);
        };

        // Add paths to search stub packages.
        add_paths(&mut paths, config_options.stub_path);
        add_paths(
            &mut paths,
            Some(exec_env.root.unwrap_or(config_options.project_root)),
        );
        exec_env
            .extra_paths
            .iter()
            .for_each(|p| add_paths(&mut paths, Some(*p)));
        add_paths(&mut paths, typeshed_path_ex);

        self.get_python_search_paths(None)
            .into_iter()
            .for_each(|p| add_paths(&mut paths, Some(p)));

        ps.process_partial_stub_packages(
            &paths,
            &self.get_import_roots(exec_env, false),
            typeshed_path_ex,
        );
        self._invalidate_file_system_cache();
        true
    }

    // analyzer/importResolver.ts:436
    /// pyright also keeps the first call's logger as `failureInfo`; nothing
    /// reads it.
    pub fn get_python_search_paths(&self, import_logger: Option<&ImportLogger>) -> Vec<Uri> {
        // Find the site packages for the configured virtual environment.
        if self._cached_python_search_paths.borrow().is_none() {
            let config_options = self._config_options.get();
            let paths: Vec<Uri> = python_path_utils::find_python_search_paths(
                self.file_system(),
                &config_options,
                self.host,
                import_logger,
                false,
                None,
            )
            .into_iter()
            .map(|p| self.file_system().real_case_path(p))
            .collect();

            // Remove duplicates (yes, it happens). pyright's `Set` compares
            // Uri objects, which match keys while `createFileUri`'s LRU holds them.
            *self._cached_python_search_paths.borrow_mut() = Some(
                paths
                    .into_iter()
                    .collect::<JsSet<Uri>>()
                    .into_iter()
                    .collect(),
            );
        }

        self._cached_python_search_paths
            .borrow()
            .clone()
            .unwrap_or_default()
    }

    // analyzer/importResolver.ts:451
    pub fn get_typeshed_stdlib_exclude_list(
        &self,
        custom_typeshed_path: Option<Uri>,
        python_version: PythonVersion,
        python_platform: Option<&str>,
    ) -> Vec<Uri> {
        let typeshed_stdlib_path = self._get_stdlib_typeshed_path(
            custom_typeshed_path,
            python_version,
            python_platform,
            None,
            None,
        );
        let mut excludes: Vec<Uri> = Vec::new();

        let Some(typeshed_stdlib_path) = typeshed_stdlib_path else {
            return excludes;
        };

        let versions = self
            ._get_typeshed_info_provider()
            .get_std_lib_module_version_info(custom_typeshed_path, None);

        versions.iter().for_each(|(module_name, version_info)| {
            let mut should_exclude_module = false;

            if let Some(max) = &version_info.max {
                if PythonVersion::is_greater_than(&python_version, max) {
                    should_exclude_module = true;
                }
            }

            if let Some(python_platform) = python_platform {
                let python_platform_lower = python_platform.js_to_lower_case();

                // If there are supported platforms listed, and we are not using one
                // of those supported platforms, exclude it.
                if let Some(supported_platforms) = &version_info.supported_platforms {
                    if supported_platforms
                        .iter()
                        .all(|p| p.js_to_lower_case() != python_platform_lower)
                    {
                        should_exclude_module = true;
                    }
                }

                // If there are unsupported platforms listed, see if we're using one of them.
                if let Some(unsupported_platforms) = &version_info.unsupported_platforms {
                    if unsupported_platforms
                        .iter()
                        .any(|p| p.js_to_lower_case() == python_platform_lower)
                    {
                        should_exclude_module = true;
                    }
                }
            }

            if should_exclude_module {
                // Add excludes for both the ".pyi" file and the directory that contains it
                // (in case it's using a "__init__.pyi" file).
                let module_name_parts: Vec<&str> = module_name.split('.').collect();
                let module_dir_path = typeshed_stdlib_path.combine_paths(&module_name_parts);
                excludes.push(module_dir_path);

                let module_file_path = module_dir_path.replace_extension(".pyi");
                excludes.push(module_file_path);
            }
        });

        excludes
    }

    // analyzer/importResolver.ts:512
    // Intended to be overridden by subclasses to provide additional stub
    // path capabilities. Return undefined if no extra stub path were found.
    pub fn get_typeshed_path_ex(
        &self,
        _exec_env: &ExecutionEnvironment,
        _import_logger: Option<&ImportLogger>,
    ) -> Option<Uri> {
        None
    }

    // analyzer/importResolver.ts:518
    // Resolves the import and returns the path if it exists, otherwise
    // returns undefined.
    fn resolve_import_internal(
        &self,
        source_file_uri: Uri,
        exec_env: &ExecutionEnvironment,
        module_descriptor: &ImportedModuleDescriptor,
    ) -> P<ImportResult> {
        let import_name = format_import_name(module_descriptor);
        let import_result = self._resolve_import_strict(
            &import_name,
            source_file_uri,
            exec_env,
            module_descriptor,
            None,
        );

        if import_result.is_import_found || module_descriptor.leading_dots > 0 {
            return import_result;
        }

        // If the import is absolute and no other method works, try resolving the
        // absolute in the importing file's directory, then the parent directory,
        // and so on, until the import root is reached.
        let origin = source_file_uri.get_directory();

        let result = self.cached_parent_import_results.get_import_result(
            origin,
            &import_name,
            import_result,
        );
        if let Some(result) = result {
            // Already ran the parent directory resolution for this import name on this location.
            return self
                .filter_implicit_imports(result, module_descriptor.imported_symbols.as_ref());
        }

        // Check whether the given file is in the parent directory import resolution cache.
        let root = get_parent_import_resolution_root(source_file_uri, exec_env.root);
        if !self.cached_parent_import_results.check_valid_path(
            self.file_system(),
            source_file_uri,
            root,
            &|| self.get_python_search_paths(None),
        ) {
            return import_result;
        }

        let import_logger = if self._config_options.get().verbose_output == Some(true) {
            Some(ImportLogger::new())
        } else {
            None
        };

        let import_path = P::new(ImportPath {
            import_path: Cell::new(None),
        });

        // Going up the given folder one by one until we can resolve the import.
        let mut current = Some(origin);
        while self._should_walk_up(current, root, exec_env)
            && let Some(cur) = current
        {
            let result = self.resolve_absolute_import(
                Some(source_file_uri),
                cur,
                exec_env,
                module_descriptor,
                &import_name,
                import_logger.as_ref(),
                /* allowPartial */ false,
                /* allowNativeLib */ false,
                /* useStubPackage */ false,
                /* allowPyi */ true,
                /* lookForPyTyped */ false,
            );

            self.cached_parent_import_results
                .checked(cur, &import_name, import_path);

            if let Some(result) = result
                && result.is_import_found
            {
                // This will make cache to point to actual path that contains the module we found
                import_path.import_path.set(Some(cur));

                let result = P::new(result);
                self.cached_parent_import_results.add(CacheEntry {
                    import_result: result,
                    path: cur,
                    import_name: import_name.clone(),
                });

                return self
                    .filter_implicit_imports(result, module_descriptor.imported_symbols.as_ref());
            }

            current = self._try_walk_up(current);
        }

        if let Some(current) = current {
            self.cached_parent_import_results
                .checked(current, &import_name, import_path);
        }

        if let Some(import_logger) = &import_logger {
            let console = self.service_provider.console();
            import_logger
                .get_logs()
                .iter()
                .for_each(|diag| console.log(diag));
        }

        import_result
    }

    // analyzer/importResolver.ts:597
    fn file_exists_cached(&self, uri: Uri) -> bool {
        self._file_system_cache.file_exists(uri)
    }

    // analyzer/importResolver.ts:601
    fn dir_exists_cached(&self, uri: Uri) -> bool {
        self._file_system_cache.dir_exists(uri)
    }

    // analyzer/importResolver.ts:605
    fn add_results_to_cache(
        &self,
        source_file_uri: Uri,
        exec_env: &ExecutionEnvironment,
        import_name: &str,
        import_result: ImportResult,
        module_descriptor: Option<&ImportedModuleDescriptor>,
        from_user_file: bool,
    ) -> P<ImportResult> {
        // If the import is relative, include the source file path in the key.
        let relative_source_file_uri = if module_descriptor.is_some_and(|md| md.leading_dots > 0) {
            Some(source_file_uri)
        } else {
            None
        };

        let import_result = P::new(import_result);
        self._cached_import_results
            .borrow_mut()
            .entry(exec_env.root.map(|r| r.key()))
            .or_default()
            .insert(
                self._get_import_cache_key(relative_source_file_uri, import_name, from_user_file),
                import_result,
            );

        self.filter_implicit_imports(
            import_result,
            module_descriptor.and_then(|md| md.imported_symbols.as_ref()),
        )
    }

    // analyzer/importResolver.ts:626
    // Follows import resolution algorithm defined in PEP-420:
    // https://www.python.org/dev/peps/pep-0420/
    fn resolve_absolute_import(
        &self,
        _source_file_uri: Option<Uri>,
        root_path: Uri,
        exec_env: &ExecutionEnvironment,
        module_descriptor: &ImportedModuleDescriptor,
        import_name: &str,
        import_logger: Option<&ImportLogger>,
        allow_partial: bool,
        allow_native_lib: bool,
        use_stub_package: bool,
        allow_pyi: bool,
        look_for_py_typed: bool,
    ) -> Option<ImportResult> {
        // Before we do additional work, see if this directory can possibly
        // resolve this import.
        if !self._is_possible_import_dir(root_path, module_descriptor) {
            return None;
        }

        if allow_pyi && use_stub_package {
            // Look for packaged stubs first. PEP 561 indicates that package authors can ship
            // their stubs separately from their package implementation by appending the string
            // '-stubs' to its top - level directory name. We'll look there first.
            let import_result = self._resolve_absolute_import(
                root_path,
                exec_env,
                module_descriptor,
                import_name,
                import_logger,
                allow_partial,
                /* allowNativeLib */ false,
                /* useStubPackage */ true,
                /* allowPyi */ true,
                /* lookForPyTyped */ true,
            );

            // We found fully typed stub packages.
            if import_result.package_directory.is_some() {
                // If this is a namespace package that wasn't resolved, assume that
                // it's a partial stub package and continue looking for a real package.
                if !import_result.is_namespace_package || import_result.is_import_found {
                    return Some(import_result);
                }
            }
        }

        Some(self._resolve_absolute_import(
            root_path,
            exec_env,
            module_descriptor,
            import_name,
            import_logger,
            allow_partial,
            allow_native_lib,
            /* useStubPackage */ false,
            allow_pyi,
            look_for_py_typed,
        ))
    }

    // analyzer/importResolver.ts:691
    // Intended to be overridden by subclasses to provide additional stub
    // resolving capabilities. Return undefined if no stubs were found for
    // this import. `bestResultSoFar` is the best import resolved so far (from
    // site-packages/local/etc.) so subclasses can decide whether their stubs
    // should override it.
    fn resolve_import_ex(
        &self,
        _source_file_uri: Uri,
        _exec_env: &ExecutionEnvironment,
        _module_descriptor: &ImportedModuleDescriptor,
        _import_name: &str,
        _best_result_so_far: Option<&ImportResult>,
        _import_logger: Option<&ImportLogger>,
        _allow_pyi: bool,
    ) -> Option<ImportResult> {
        None
    }

    // analyzer/importResolver.ts:706
    // Intended to be overridden by subclasses to provide additional stub
    // resolving capabilities for native (compiled) modules. Returns undefined
    // if no stubs were found for this import.
    fn resolve_native_import_ex(
        &self,
        _library_file_uri: Uri,
        _import_name: &str,
        _import_logger: Option<&ImportLogger>,
    ) -> Option<Uri> {
        None
    }

    // analyzer/importResolver.ts:724
    // Potentially modifies the ImportResult by removing some or all of the
    // implicit import entries. Only the imported symbols should be included.
    fn filter_implicit_imports(
        &self,
        import_result: P<ImportResult>,
        imported_symbols: Option<&JsSet<String>>,
    ) -> P<ImportResult> {
        let Some(imported_symbols) = imported_symbols else {
            let mut new_import_result = (*import_result).clone();
            new_import_result.filtered_implicit_imports = None;
            return P::new(new_import_result);
        };

        if imported_symbols.is_empty() {
            return import_result;
        }

        let Some(implicit_imports) = import_result
            .implicit_imports
            .as_ref()
            .filter(|m| !m.is_empty())
        else {
            return import_result;
        };

        let mut filtered_implicit_imports: JsMap<String, ImplicitImport> = JsMap::default();
        implicit_imports.values().for_each(|implicit_import| {
            if imported_symbols.contains(&implicit_import.name) {
                filtered_implicit_imports
                    .insert(implicit_import.name.clone(), implicit_import.clone());
            }
        });

        if filtered_implicit_imports.len() == implicit_imports.len() {
            return import_result;
        }

        let mut new_import_result = (*import_result).clone();
        new_import_result.filtered_implicit_imports = Some(filtered_implicit_imports);
        P::new(new_import_result)
    }

    // analyzer/importResolver.ts:758
    fn find_implicit_imports(
        &self,
        importing_module_name: &str,
        dir_path: Uri,
        exclusions: &[Uri],
    ) -> Option<JsMap<String, ImplicitImport>> {
        let mut implicit_import_map: JsMap<String, ImplicitImport> = JsMap::default();

        // Enumerate all of the files and directories in the path, expanding links.
        let entries = get_file_system_entries_from_dir_entries(
            self._file_system_cache
                .readdir_entries_sync(dir_path)
                .to_vec(),
            self.file_system(),
            dir_path,
        );

        // Add implicit file-based modules.
        for file_path in entries.files {
            let file_ext = file_path.last_extension();
            let stripped_file_name: String;
            let mut is_native_lib = false;

            if file_ext == ".py" || file_ext == ".pyi" {
                stripped_file_name = strip_file_extension(file_path.file_name(), false);
            } else if _is_native_module_file_extension(file_ext)
                && !self.file_exists_cached(file_path.package_uri())
                && !self.file_exists_cached(file_path.package_stub_uri())
            {
                // Native module.
                stripped_file_name = file_path.strip_all_extensions().file_name().to_string();
                is_native_lib = true;
            } else {
                continue;
            }

            if !exclusions
                .iter()
                .any(|exclusion| exclusion.equals(Some(file_path)))
            {
                let mut implicit_import = ImplicitImport {
                    is_stub_file: file_path.has_extension(".pyi"),
                    is_native_lib,
                    name: stripped_file_name.clone(),
                    uri: file_path,
                    py_typed_info: None,
                };

                // Always prefer stub files over non-stub files.
                let entry_is_stub_file = implicit_import_map
                    .get(&implicit_import.name)
                    .map(|entry| entry.is_stub_file);
                if entry_is_stub_file != Some(true) {
                    // Try resolving resolving native lib to a custom stub.
                    if is_native_lib {
                        let native_lib_path = file_path;
                        let native_stub_path = self.resolve_native_import_ex(
                            native_lib_path,
                            &format!("{importing_module_name}.{stripped_file_name}"),
                            None,
                        );
                        if let Some(native_stub_path) = native_stub_path {
                            implicit_import.uri = native_stub_path;
                            implicit_import.is_native_lib = false;
                        }
                    }
                    implicit_import_map.insert(implicit_import.name.clone(), implicit_import);
                }
            }
        }

        // Add implicit directory-based modules.
        for dir_path in entries.directories {
            let py_file_path = dir_path.init_py_uri();
            let pyi_file_path = dir_path.init_pyi_uri();
            let mut is_stub_file = false;
            let mut path: Option<Uri> = None;

            if self.file_exists_cached(pyi_file_path) {
                is_stub_file = true;
                path = Some(pyi_file_path);
            } else if self.file_exists_cached(py_file_path) {
                path = Some(py_file_path);
            }

            if let Some(path) = path {
                if !exclusions
                    .iter()
                    .any(|exclusion| exclusion.equals(Some(path)))
                {
                    let implicit_import = ImplicitImport {
                        is_stub_file,
                        is_native_lib: false,
                        name: dir_path.file_name().to_string(),
                        uri: path,
                        py_typed_info: self._get_py_typed_info(dir_path),
                    };

                    implicit_import_map.insert(implicit_import.name.clone(), implicit_import);
                }
            }
        }

        if !implicit_import_map.is_empty() {
            Some(implicit_import_map)
        } else {
            None
        }
    }

    // analyzer/importResolver.ts:852
    fn _is_possible_import_dir(
        &self,
        root_path: Uri,
        module_descriptor: &ImportedModuleDescriptor,
    ) -> bool {
        let resolvable_names = self
            ._file_system_cache
            .get_resolvable_names_in_directory(root_path);

        let is_potential_match = |name: &str| -> bool { resolvable_names.contains(name) };

        if !module_descriptor.name_parts.is_empty() {
            return is_potential_match(&module_descriptor.name_parts[0]);
        }

        if let Some(imported_symbols) = &module_descriptor.imported_symbols {
            for key in imported_symbols {
                if is_potential_match(key) {
                    return true;
                }
            }
        }

        is_potential_match("__init__")
    }

    // analyzer/importResolver.ts:874
    fn _resolve_import_strict(
        &self,
        import_name: &str,
        source_file_uri: Uri,
        exec_env: &ExecutionEnvironment,
        module_descriptor: &ImportedModuleDescriptor,
        import_logger: Option<&ImportLogger>,
    ) -> P<ImportResult> {
        let from_user_file = match_file_specs(&self._config_options.get(), source_file_uri, true);
        let not_found_result = ImportResult {
            import_name: import_name.to_string(),
            is_relative: false,
            is_import_found: false,
            is_partly_resolved: false,
            is_namespace_package: false,
            is_init_file_present: false,
            is_stub_package: false,
            import_failure_info: import_logger.map(|l| l.get_logs()),
            resolved_uris: Vec::new(),
            import_type: ImportType::Local,
            is_stub_file: false,
            is_native_lib: false,
            implicit_imports: None,
            filtered_implicit_imports: None,
            non_stub_import_result: None,
            search_path: None,
            is_stdlib_typeshed_file: None,
            is_third_party_typeshed_file: None,
            is_local_typings_file: None,
            py_typed_info: None,
            package_directory: None,
        };

        self.ensure_partial_stub_packages(exec_env);

        // Is it a relative import?
        if module_descriptor.leading_dots > 0 {
            let cached_results = self._look_up_results_in_cache(
                source_file_uri,
                exec_env,
                import_name,
                module_descriptor,
                from_user_file,
            );

            if let Some(cached_results) = cached_results {
                return cached_results;
            }

            let relative_import = self._resolve_relative_import(
                source_file_uri,
                exec_env,
                module_descriptor,
                import_name,
                import_logger,
            );

            if let Some(mut relative_import) = relative_import {
                relative_import.is_relative = true;

                return self.add_results_to_cache(
                    source_file_uri,
                    exec_env,
                    import_name,
                    relative_import,
                    Some(module_descriptor),
                    from_user_file,
                );
            }
        } else {
            let cached_results = self._look_up_results_in_cache(
                source_file_uri,
                exec_env,
                import_name,
                module_descriptor,
                from_user_file,
            );

            if let Some(cached_results) = cached_results {
                // In most cases, we can simply return a cached entry. However, there are cases
                // where the cached entry refers to a previously-resolved namespace package
                // that does not resolve the symbols specified in the module descriptor.
                // In this case, we will ignore the cached value and run the full import
                // resolution again to try to find a package that resolves the import.
                let is_unresolved_namespace = cached_results.is_import_found
                    && cached_results.is_namespace_package
                    && !self._is_namespace_package_resolved(
                        module_descriptor,
                        cached_results.implicit_imports.as_ref(),
                    );

                if !is_unresolved_namespace {
                    return cached_results;
                }
            }

            let best_import = self._resolve_best_absolute_import(
                source_file_uri,
                exec_env,
                module_descriptor,
                /* allowPyi */ true,
            );

            if let Some(mut best_import) = best_import {
                if best_import.is_stub_file {
                    best_import.non_stub_import_result = Some(P::new(
                        self._resolve_best_absolute_import(
                            source_file_uri,
                            exec_env,
                            module_descriptor,
                            /* allowPyi */ false,
                        )
                        .unwrap_or(not_found_result),
                    ));
                }

                return self.add_results_to_cache(
                    source_file_uri,
                    exec_env,
                    import_name,
                    best_import,
                    Some(module_descriptor),
                    from_user_file,
                );
            }
        }

        self.add_results_to_cache(
            source_file_uri,
            exec_env,
            import_name,
            not_found_result,
            /* moduleDescriptor */ None,
            from_user_file,
        )
    }

    // analyzer/importResolver.ts:1067
    fn _get_module_name_for_import(
        &self,
        file_uri: Uri,
        exec_env: &ExecutionEnvironment,
        allow_invalid_module_name: bool,
        detect_py_typed: bool,
    ) -> ModuleImportInfo {
        let config_options = self._config_options.get();
        let mut module_name: Option<String> = None;
        let mut import_type = ImportType::BuiltIn;
        let mut is_local_typings_file = false;
        let mut is_third_party_py_typed_present = false;
        let mut is_typeshed_file = false;

        // A JS string is truthy when it is not empty; `length` counts UTF-16 units.
        let is_truthy = |s: &Option<String>| s.as_deref().is_some_and(|s| !s.is_empty());
        let js_len = |s: &Option<String>| s.as_deref().map_or(0, |s| s.js_len());

        // If we cannot find a fully-qualified module name with legal characters,
        // look for one with invalid characters (e.g. "-"). This is important to
        // differentiate between different modules in a project in case they
        // declare types with the same (local) name.
        let mut module_name_with_invalid_characters: Option<String> = None;

        // Is this a stdlib typeshed path?
        let std_lib_typeshed_path = self._get_stdlib_typeshed_path(
            config_options.typeshed_path,
            exec_env.python_version,
            exec_env.python_platform.as_deref(),
            None,
            None,
        );

        if let Some(std_lib_typeshed_path) = std_lib_typeshed_path {
            module_name = get_module_name_from_path(std_lib_typeshed_path, file_uri, false);
            if let Some(module_name) = module_name.as_ref().filter(|m| !m.is_empty()) {
                let module_descriptor = ImportedModuleDescriptor {
                    leading_dots: 0,
                    name_parts: module_name.split('.').map(str::to_string).collect(),
                    has_trailing_dot: None,
                    imported_symbols: None,
                };

                if self._is_stdlib_typeshed_stub_valid_for_version(
                    &module_descriptor,
                    config_options.typeshed_path,
                    exec_env.python_version,
                    exec_env.python_platform.as_deref(),
                    None,
                ) {
                    return ModuleImportInfo {
                        module_name: module_name.clone(),
                        import_type,
                        is_typeshed_file: true,
                        is_local_typings_file,
                        is_third_party_py_typed_present,
                    };
                }
            }
        }

        // Look for it in the root directory of the execution environment.
        if let Some(root) = exec_env.root {
            let candidate_module_name_info = _get_module_name_info_from_path(root, file_uri, false);

            if let Some(candidate_module_name_info) = candidate_module_name_info {
                if candidate_module_name_info.contains_invalid_characters == Some(true) {
                    module_name_with_invalid_characters =
                        Some(candidate_module_name_info.module_name);
                } else {
                    module_name = Some(candidate_module_name_info.module_name);
                }
            }

            import_type = ImportType::Local;
        }

        for extra_path in &exec_env.extra_paths {
            let candidate_module_name_info =
                _get_module_name_info_from_path(*extra_path, file_uri, false);

            if let Some(candidate_module_name_info) = candidate_module_name_info {
                if candidate_module_name_info.contains_invalid_characters == Some(true) {
                    module_name_with_invalid_characters =
                        Some(candidate_module_name_info.module_name);
                } else {
                    // Does this candidate look better than the previous best module name?
                    // We'll always try to use the shortest version.
                    let candidate_module_name = Some(candidate_module_name_info.module_name);
                    if !is_truthy(&module_name)
                        || (is_truthy(&candidate_module_name)
                            && js_len(&candidate_module_name) < js_len(&module_name))
                    {
                        module_name = candidate_module_name;
                        import_type = ImportType::Local;
                    }
                }
            }
        }

        // Check for a typings file.
        if let Some(stub_path) = config_options.stub_path {
            let candidate_module_name_info =
                _get_module_name_info_from_path(stub_path, file_uri, false);

            if let Some(candidate_module_name_info) = candidate_module_name_info {
                if candidate_module_name_info.contains_invalid_characters == Some(true) {
                    module_name_with_invalid_characters =
                        Some(candidate_module_name_info.module_name);
                } else {
                    // Does this candidate look better than the previous best module name?
                    // We'll always try to use the shortest version.
                    let candidate_module_name = Some(candidate_module_name_info.module_name);
                    if !is_truthy(&module_name)
                        || (is_truthy(&candidate_module_name)
                            && js_len(&candidate_module_name) < js_len(&module_name))
                    {
                        module_name = candidate_module_name;

                        // Treat the typings path as a local import so errors are reported for it.
                        import_type = ImportType::Local;
                        is_local_typings_file = true;
                    }
                }
            }
        }

        // Check for a typeshed file.
        let third_party_typeshed_path =
            self._get_third_party_typeshed_path(config_options.typeshed_path, None);

        if let Some(third_party_typeshed_path) = third_party_typeshed_path {
            let candidate_module_name = get_module_name_from_path(
                third_party_typeshed_path,
                file_uri,
                /* stripTopContainerDir */ true,
            );

            // Does this candidate look better than the previous best module name?
            // We'll always try to use the shortest version.
            if !is_truthy(&module_name)
                || (is_truthy(&candidate_module_name)
                    && js_len(&candidate_module_name) < js_len(&module_name))
            {
                module_name = candidate_module_name;
                import_type = ImportType::ThirdParty;
                is_typeshed_file = true;
            }
        }

        let third_party_typeshed_path_ex = self.get_typeshed_path_ex(exec_env, None);
        if let Some(third_party_typeshed_path_ex) = third_party_typeshed_path_ex {
            let candidate_module_name =
                get_module_name_from_path(third_party_typeshed_path_ex, file_uri, false);

            // Does this candidate look better than the previous best module name?
            // We'll always try to use the shortest version.
            if !is_truthy(&module_name)
                || (is_truthy(&candidate_module_name)
                    && js_len(&candidate_module_name) < js_len(&module_name))
            {
                module_name = candidate_module_name;
                import_type = ImportType::ThirdParty;
                is_typeshed_file = true;
            }
        }

        // Look for the import in the list of third-party packages.
        let python_search_paths = self.get_python_search_paths(None);

        for search_path in python_search_paths {
            let candidate_module_name_info =
                _get_module_name_info_from_path(search_path, file_uri, false);

            if let Some(candidate_module_name_info) = candidate_module_name_info {
                if candidate_module_name_info.contains_invalid_characters == Some(true) {
                    module_name_with_invalid_characters =
                        Some(candidate_module_name_info.module_name);
                } else {
                    // Does this candidate look better than the previous best module name?
                    // We'll always try to use the shortest version.
                    let candidate_module_name = Some(candidate_module_name_info.module_name);
                    if !is_truthy(&module_name)
                        || (is_truthy(&candidate_module_name)
                            && js_len(&candidate_module_name) < js_len(&module_name))
                    {
                        module_name = candidate_module_name;
                        import_type = ImportType::ThirdParty;
                        is_typeshed_file = false;
                    }
                }
            }
        }

        if detect_py_typed && import_type == ImportType::ThirdParty {
            let root = get_parent_import_resolution_root(file_uri, exec_env.root);

            // Go up directories one by one looking for a py.typed file.
            let mut current = Some(file_uri.get_directory());
            while self._should_walk_up(current, root, exec_env) {
                let py_typed_info = self._get_py_typed_info(current.unwrap());
                if let Some(py_typed_info) = py_typed_info {
                    if !py_typed_info.is_partially_typed {
                        is_third_party_py_typed_present = true;
                    }
                    break;
                }

                current = self._try_walk_up(current);
            }
        }

        if is_truthy(&module_name) {
            return ModuleImportInfo {
                module_name: module_name.unwrap_or_default(),
                import_type,
                is_typeshed_file,
                is_local_typings_file,
                is_third_party_py_typed_present,
            };
        }

        if allow_invalid_module_name && is_truthy(&module_name_with_invalid_characters) {
            return ModuleImportInfo {
                module_name: module_name_with_invalid_characters.unwrap_or_default(),
                is_typeshed_file,
                import_type,
                is_local_typings_file,
                is_third_party_py_typed_present,
            };
        }

        // We didn't find any module name.
        ModuleImportInfo {
            module_name: String::new(),
            is_typeshed_file,
            import_type: ImportType::Local,
            is_local_typings_file,
            is_third_party_py_typed_present,
        }
    }

    // analyzer/importResolver.ts:1271
    fn _invalidate_file_system_cache(&self) {
        self._file_system_cache.invalidate_cache();
    }

    // analyzer/importResolver.ts:1275
    fn _resolve_absolute_import(
        &self,
        root_path: Uri,
        exec_env: &ExecutionEnvironment,
        module_descriptor: &ImportedModuleDescriptor,
        import_name: &str,
        import_logger: Option<&ImportLogger>,
        allow_partial: bool,
        allow_native_lib: bool,
        use_stub_package: bool,
        allow_pyi: bool,
        look_for_py_typed: bool,
    ) -> ImportResult {
        if let Some(logger) = import_logger {
            if use_stub_package {
                logger.log(&format!(
                    "Attempting to resolve stub package using root path '{}'",
                    root_path.to_uri_string()
                ));
            } else {
                logger.log(&format!(
                    "Attempting to resolve using root path '{}'",
                    root_path.to_uri_string()
                ));
            }
        }

        // Starting at the specified path, walk the file system to find the
        // specified module.
        let mut resolved_paths: Vec<Uri> = Vec::new();
        let mut dir_path = root_path;
        let mut is_namespace_package = false;
        let mut is_init_file_present = false;
        let mut is_stub_package = false;
        let mut is_stub_file = false;
        let mut is_native_lib = false;
        let mut implicit_imports: Option<JsMap<String, ImplicitImport>> = None;
        let mut package_directory: Option<Uri> = None;
        let mut py_typed_info: Option<PyTypedInfo> = None;

        let log = |message: &dyn Fn() -> String| {
            if let Some(logger) = import_logger {
                logger.log(&message());
            }
        };

        // Handle the "from . import XXX" case.
        if module_descriptor.name_parts.is_empty() {
            let py_file_path = dir_path.init_py_uri();
            let pyi_file_path = dir_path.init_pyi_uri();

            if allow_pyi && self.file_exists_cached(pyi_file_path) {
                log(&|| {
                    format!(
                        "Resolved import with file '{}'",
                        pyi_file_path.to_uri_string()
                    )
                });
                resolved_paths.push(pyi_file_path);
                is_stub_file = true;
            } else if self.file_exists_cached(py_file_path) {
                log(&|| {
                    format!(
                        "Resolved import with file '{}'",
                        py_file_path.to_uri_string()
                    )
                });
                resolved_paths.push(py_file_path);
            } else {
                log(&|| {
                    format!(
                        "Partially resolved import with directory '{}'",
                        dir_path.to_uri_string()
                    )
                });
                resolved_paths.push(Uri::empty());
                is_namespace_package = true;
            }

            implicit_imports =
                self.find_implicit_imports(import_name, dir_path, &[py_file_path, pyi_file_path]);
        } else {
            for i in 0..module_descriptor.name_parts.len() {
                let is_first_part = i == 0;
                let is_last_part = i == module_descriptor.name_parts.len() - 1;
                dir_path = dir_path.combine_paths(&[&module_descriptor.name_parts[i]]);

                if use_stub_package && is_first_part {
                    dir_path = dir_path.add_path(STUBS_SUFFIX);
                    is_stub_package = true;
                }

                let found_directory = self.dir_exists_cached(dir_path);

                if found_directory {
                    if is_first_part {
                        package_directory = Some(dir_path);
                    }

                    // See if we can find an __init__.py[i] in this directory.
                    let py_file_path = dir_path.init_py_uri();
                    let pyi_file_path = dir_path.init_pyi_uri();
                    is_init_file_present = false;

                    if allow_pyi && self.file_exists_cached(pyi_file_path) {
                        log(&|| {
                            format!(
                                "Resolved import with file '{}'",
                                pyi_file_path.to_uri_string()
                            )
                        });
                        resolved_paths.push(pyi_file_path);
                        if is_last_part {
                            is_stub_file = true;
                        }
                        is_init_file_present = true;
                    } else if self.file_exists_cached(py_file_path) {
                        log(&|| {
                            format!(
                                "Resolved import with file '{}'",
                                py_file_path.to_uri_string()
                            )
                        });
                        resolved_paths.push(py_file_path);
                        is_init_file_present = true;
                    }

                    if py_typed_info.is_none() && look_for_py_typed {
                        py_typed_info = self._get_py_typed_info(dir_path);
                    }

                    if is_init_file_present {
                        if !is_last_part {
                            // We are not at the last part, and we found a directory,
                            // so continue to look for the next part.
                            continue;
                        }

                        implicit_imports = self.find_implicit_imports(
                            &module_descriptor.name_parts.join("."),
                            dir_path,
                            &[py_file_path, pyi_file_path],
                        );
                        break;
                    }
                }

                // We weren't able to find a directory or we found a directory with
                // no __init__.py[i] file. See if we can find a ".py" or ".pyi" file
                // with this name.
                let py_file_path = dir_path.package_uri();
                let pyi_file_path = dir_path.package_stub_uri();
                let file_directory = dir_path.get_directory();

                if allow_pyi && self.file_exists_cached(pyi_file_path) {
                    log(&|| {
                        format!(
                            "Resolved import with file '{}'",
                            pyi_file_path.to_uri_string()
                        )
                    });
                    resolved_paths.push(pyi_file_path);
                    if is_last_part {
                        is_stub_file = true;
                    }
                } else if self.file_exists_cached(py_file_path) {
                    log(&|| {
                        format!(
                            "Resolved import with file '{}'",
                            py_file_path.to_uri_string()
                        )
                    });
                    resolved_paths.push(py_file_path);
                } else if allow_native_lib
                    && self._find_and_resolve_native_module(
                        file_directory,
                        dir_path,
                        exec_env,
                        import_name,
                        module_descriptor,
                        import_logger,
                        &mut resolved_paths,
                    )
                {
                    is_native_lib = true;
                    log(&|| {
                        format!(
                            "Did not find file '{}' or '{}'",
                            pyi_file_path.to_uri_string(),
                            py_file_path.to_uri_string()
                        )
                    });
                } else if !allow_pyi
                    && allow_native_lib
                    && is_last_part
                    && self.file_exists_cached(dir_path.add_extension(".pyc"))
                {
                    // Sourceless distribution: a compiled `.pyc` module sits where the `.py`
                    // source would be (e.g. `mod.pyc` beside `mod.pyi`, with no `mod.py`) -- placed
                    // directly in the package directory, not under `__pycache__`. Python imports
                    // such a `.pyc` directly, so the module exists at runtime even though it has no
                    // Python source. Only the non-stub resolution (`allowPyi === false`) reaches
                    // this branch, and only where native libs are allowed.
                    let pyc_file_path = dir_path.add_extension(".pyc");
                    log(&|| {
                        format!(
                            "Resolved import with compiled file '{}'",
                            pyc_file_path.to_uri_string()
                        )
                    });
                    resolved_paths.push(pyc_file_path);
                    is_native_lib = true;
                } else if found_directory {
                    if !is_last_part {
                        // We are not at the last part, and we found a directory,
                        // so continue to look for the next part assuming this is
                        // a namespace package.
                        resolved_paths.push(Uri::empty());
                        is_namespace_package = true;
                        py_typed_info = None;
                        continue;
                    }

                    log(&|| {
                        format!(
                            "Partially resolved import with directory '{}'",
                            dir_path.to_uri_string()
                        )
                    });
                    resolved_paths.push(Uri::empty());

                    if is_last_part {
                        implicit_imports = self.find_implicit_imports(
                            import_name,
                            dir_path,
                            &[py_file_path, pyi_file_path],
                        );
                        is_namespace_package = true;
                    }
                }

                if py_typed_info.is_none() && look_for_py_typed {
                    py_typed_info = self._get_py_typed_info(file_directory);
                }
                break;
            }
        }

        let is_partly_resolved =
            !resolved_paths.is_empty() && resolved_paths.len() < module_descriptor.name_parts.len();
        let import_found = if allow_partial {
            !resolved_paths.is_empty()
        } else {
            resolved_paths.len() >= module_descriptor.name_parts.len()
        };

        ImportResult {
            import_name: import_name.to_string(),
            is_relative: false,
            is_namespace_package,
            is_init_file_present,
            is_stub_package,
            is_import_found: import_found,
            is_partly_resolved,
            import_failure_info: import_logger.map(|l| l.get_logs()),
            import_type: ImportType::Local,
            resolved_uris: resolved_paths,
            search_path: Some(root_path),
            is_stub_file,
            is_native_lib,
            filtered_implicit_imports: implicit_imports.clone(),
            implicit_imports,
            py_typed_info,
            package_directory,
            is_stdlib_typeshed_file: None,
            is_third_party_typeshed_file: None,
            is_local_typings_file: None,
            non_stub_import_result: None,
        }
    }

    // analyzer/importResolver.ts:1502
    fn _get_import_cache_key(
        &self,
        source_file_uri: Option<Uri>,
        import_name: &str,
        from_user_file: bool,
    ) -> String {
        format!(
            "{}-{import_name}-{from_user_file}",
            source_file_uri.map_or("", |u| u.key())
        )
    }

    // analyzer/importResolver.ts:1506
    fn _look_up_results_in_cache(
        &self,
        source_file_uri: Uri,
        exec_env: &ExecutionEnvironment,
        import_name: &str,
        module_descriptor: &ImportedModuleDescriptor,
        from_user_file: bool,
    ) -> Option<P<ImportResult>> {
        // If the import is relative, include the source file path in the key.
        let relative_source_file_uri = if module_descriptor.leading_dots > 0 {
            Some(source_file_uri)
        } else {
            None
        };

        let cached_entry = {
            let cached_import_results = self._cached_import_results.borrow();
            let cache_for_exec_env =
                cached_import_results.get(&Some(exec_env.root.map_or("", |r| r.key())))?;

            cache_for_exec_env
                .get(&self._get_import_cache_key(
                    relative_source_file_uri,
                    import_name,
                    from_user_file,
                ))
                .copied()
        };

        let cached_entry = cached_entry?;

        Some(
            self.filter_implicit_imports(cached_entry, module_descriptor.imported_symbols.as_ref()),
        )
    }

    // analyzer/importResolver.ts:1537
    // Determines whether a namespace package resolves all of the symbols
    // requested in the module descriptor. Namespace packages have no "__init__.py"
    // file, so the only way that symbols can be resolved is if submodules
    // are present. If specific symbols were requested, make sure they
    // are all satisfied by submodules (as listed in the implicit imports).
    fn _is_namespace_package_resolved(
        &self,
        module_descriptor: &ImportedModuleDescriptor,
        implicit_imports: Option<&JsMap<String, ImplicitImport>>,
    ) -> bool {
        if let Some(imported_symbols) = &module_descriptor.imported_symbols {
            if !imported_symbols
                .iter()
                .any(|symbol| implicit_imports.is_some_and(|m| m.contains_key(symbol)))
            {
                return false;
            }
        } else if implicit_imports.is_none_or(|m| m.is_empty()) {
            return false;
        }
        true
    }

    // analyzer/importResolver.ts:1551
    fn _resolve_best_absolute_import(
        &self,
        source_file_uri: Uri,
        exec_env: &ExecutionEnvironment,
        module_descriptor: &ImportedModuleDescriptor,
        allow_pyi: bool,
    ) -> Option<ImportResult> {
        let config_options = self._config_options.get();
        let import_name = format_import_name(module_descriptor);
        let import_logger = if config_options.verbose_output == Some(true) {
            Some(ImportLogger::new())
        } else {
            None
        };
        let import_logger = import_logger.as_ref();
        let log = |message: &dyn Fn() -> String| {
            if let Some(logger) = import_logger {
                logger.log(&message());
            }
        };

        // In pyright `importFailureInfo` is the logger's own array
        // (`getLogs()` returns it), so a returned result holds every line
        // logged until this function returns.
        let with_final_logs = |result: Option<ImportResult>| -> Option<ImportResult> {
            result.map(|mut result| {
                if let Some(logger) = import_logger
                    && result.import_failure_info.is_some()
                {
                    result.import_failure_info = Some(logger.get_logs());
                }
                result
            })
        };

        // Check for a local stub file using stubPath.
        if allow_pyi && let Some(stub_path) = config_options.stub_path {
            log(&|| format!("Looking in stubPath '{}'", stub_path.to_uri_string()));
            let typings_import = self.resolve_absolute_import(
                Some(source_file_uri),
                stub_path,
                exec_env,
                module_descriptor,
                &import_name,
                import_logger,
                /* allowPartial */ false,
                /* allowNativeLib */ false,
                /* useStubPackage */ true,
                allow_pyi,
                /* lookForPyTyped */ false,
            );

            if let Some(mut typings_import) = typings_import
                && typings_import.is_import_found
            {
                // We will treat typings files as "local" rather than "third party".
                typings_import.import_type = ImportType::Local;
                typings_import.is_local_typings_file = Some(true);

                // If it's a namespace package that didn't resolve to a file, make sure that
                // the imported symbols are present in the implicit imports. If not, we'll
                // skip the typings import and continue searching.
                if typings_import.is_namespace_package
                    && typings_import.resolved_uris[typings_import.resolved_uris.len() - 1]
                        .is_empty()
                {
                    if self._is_namespace_package_resolved(
                        module_descriptor,
                        typings_import.implicit_imports.as_ref(),
                    ) {
                        return with_final_logs(Some(typings_import));
                    }
                } else {
                    return with_final_logs(Some(typings_import));
                }
            }
        }

        let mut best_result_so_far: Option<ImportResult> = None;

        // Look for it in the root directory of the execution environment.
        if let Some(root) = exec_env.root {
            log(&|| {
                format!(
                    "Looking in root directory of execution environment '{}'",
                    root.to_uri_string()
                )
            });

            let local_import = self.resolve_absolute_import(
                Some(source_file_uri),
                root,
                exec_env,
                module_descriptor,
                &import_name,
                import_logger,
                /* allowPartial */ false,
                /* allowNativeLib */ true,
                /* useStubPackage */ true,
                allow_pyi,
                /* lookForPyTyped */ false,
            );
            best_result_so_far = local_import;
        }

        for &extra_path in &exec_env.extra_paths {
            log(&|| format!("Looking in extraPath '{}'", extra_path.to_uri_string()));
            let local_import = self.resolve_absolute_import(
                Some(source_file_uri),
                extra_path,
                exec_env,
                module_descriptor,
                &import_name,
                import_logger,
                /* allowPartial */ false,
                /* allowNativeLib */ true,
                /* useStubPackage */ true,
                allow_pyi,
                /* lookForPyTyped */ false,
            );
            best_result_so_far =
                self._pick_best_import(best_result_so_far, local_import, module_descriptor);
        }

        // Check for a stdlib typeshed file.
        if allow_pyi && !module_descriptor.name_parts.is_empty() {
            log(&|| "Looking for typeshed stdlib path".to_string());
            let typeshed_stdlib_import = self._find_typeshed_path(
                exec_env,
                module_descriptor,
                &import_name,
                /* isStdLib */ true,
                import_logger,
            );

            if let Some(mut typeshed_stdlib_import) = typeshed_stdlib_import {
                typeshed_stdlib_import.is_stdlib_typeshed_file = Some(true);
                return with_final_logs(Some(typeshed_stdlib_import));
            }
        }

        // Look for the import in the list of third-party packages.
        let mut third_party_non_stub_import_found = false;
        let python_search_paths = self.get_python_search_paths(import_logger);
        if !python_search_paths.is_empty() {
            for search_path in python_search_paths {
                log(&|| {
                    format!(
                        "Looking in python search path '{}'",
                        search_path.to_uri_string()
                    )
                });

                let third_party_import = self.resolve_absolute_import(
                    Some(source_file_uri),
                    search_path,
                    exec_env,
                    module_descriptor,
                    &import_name,
                    import_logger,
                    /* allowPartial */ ALLOW_PARTIAL_RESOLUTION_FOR_THIRD_PARTY_PACKAGES,
                    /* allowNativeLib */ true,
                    /* useStubPackage */ true,
                    allow_pyi,
                    /* lookForPyTyped */ true,
                );

                if let Some(mut third_party_import) = third_party_import {
                    third_party_import.import_type = ImportType::ThirdParty;

                    if !third_party_import.is_stub_file
                        && (third_party_import.is_import_found
                            || third_party_import.is_partly_resolved)
                    {
                        third_party_non_stub_import_found = true;
                    }

                    best_result_so_far = self._pick_best_import(
                        best_result_so_far,
                        Some(third_party_import),
                        module_descriptor,
                    );
                }
            }
        } else {
            log(&|| "No python interpreter search path".to_string());
        }

        // If a library is fully py.typed, then we have found the best match,
        // unless the execution environment is typeshed itself, in which case
        // we don't want to favor py.typed libraries. Use the typeshed lookup below.
        // pyright compares the Uri objects with `!==`, which matches keys
        // while `createFileUri`'s LRU holds them.
        if exec_env.root
            != self
                ._get_typeshed_info_provider()
                .get_typeshed_root(config_options.typeshed_path, import_logger)
        {
            if best_result_so_far
                .as_ref()
                .is_some_and(|b| b.py_typed_info.is_some() && !b.is_partly_resolved)
            {
                return with_final_logs(best_result_so_far);
            }
        }

        // Call the extensibility hook for subclasses.
        let extra_results = self.resolve_import_ex(
            source_file_uri,
            exec_env,
            module_descriptor,
            &import_name,
            best_result_so_far.as_ref(),
            import_logger,
            allow_pyi,
        );

        if extra_results.is_some() {
            return with_final_logs(extra_results);
        }

        // Results from resolveImportEx return above, so this fallback guard applies only to typeshed fallback imports.
        // Check for a third-party typeshed file.
        if allow_pyi && !module_descriptor.name_parts.is_empty() {
            log(&|| "Looking for typeshed third-party path".to_string());
            let typeshed_import = self._find_typeshed_path(
                exec_env,
                module_descriptor,
                &import_name,
                /* isStdLib */ false,
                import_logger,
            );

            if let Some(mut typeshed_import) = typeshed_import {
                typeshed_import.is_third_party_typeshed_file = Some(true);

                if self._should_skip_third_party_typeshed_fallback_for_local_namespace(
                    best_result_so_far.as_ref(),
                    third_party_non_stub_import_found,
                ) {
                    log(&|| {
                        "Skipping typeshed third-party fallback because a local namespace package was resolved \
                         and no non-stub third-party package was found"
                            .to_string()
                    });
                } else {
                    best_result_so_far = self._pick_best_import(
                        best_result_so_far,
                        Some(typeshed_import),
                        module_descriptor,
                    );
                }
            }
        }

        // We weren't able to find an exact match, so return the best
        // partial match.
        with_final_logs(best_result_so_far)
    }

    // analyzer/importResolver.ts:1757
    fn _should_skip_third_party_typeshed_fallback_for_local_namespace(
        &self,
        best_import_so_far: Option<&ImportResult>,
        third_party_non_stub_import_found: bool,
    ) -> bool {
        !third_party_non_stub_import_found
            && best_import_so_far.is_some_and(|best_import_so_far| {
                best_import_so_far.import_type == ImportType::Local
                    && best_import_so_far.is_namespace_package
                    && best_import_so_far.is_import_found
                    && !best_import_so_far.is_stub_file
            })
    }

    // analyzer/importResolver.ts:1771
    fn _pick_best_import(
        &self,
        best_import_so_far: Option<ImportResult>,
        new_import: Option<ImportResult>,
        module_descriptor: &ImportedModuleDescriptor,
    ) -> Option<ImportResult> {
        let Some(best_import_so_far) = best_import_so_far else {
            return new_import;
        };

        let Some(new_import) = new_import else {
            return Some(best_import_so_far);
        };

        // The TypeScript body, returning `true` where it returns `newImport`.
        let pick_new_import = (|| -> bool {
            let first_non_empty = |r: &ImportResult| -> i32 {
                r.resolved_uris
                    .iter()
                    .position(|path| !path.is_empty())
                    .map_or(-1, |i| i as i32)
            };

            if new_import.is_import_found {
                // Prefer traditional packages over namespace packages.
                let so_far_index = first_non_empty(&best_import_so_far);
                let new_index = first_non_empty(&new_import);
                if so_far_index != new_index {
                    if so_far_index < 0 {
                        return true;
                    } else if new_index < 0 {
                        return false;
                    }
                    return so_far_index >= new_index;
                }

                // Prefer found over not found.
                if !best_import_so_far.is_import_found {
                    return true;
                }

                // If both are namespace imports, select the one that resolves the symbols.
                if best_import_so_far.is_namespace_package && new_import.is_namespace_package {
                    if module_descriptor.imported_symbols.is_some() {
                        if !self._is_namespace_package_resolved(
                            module_descriptor,
                            best_import_so_far.implicit_imports.as_ref(),
                        ) {
                            if self._is_namespace_package_resolved(
                                module_descriptor,
                                new_import.implicit_imports.as_ref(),
                            ) {
                                return true;
                            }

                            // Prefer the namespace package that has an __init__.py(i) file present
                            // in the final directory over one that does not.
                            if best_import_so_far.is_init_file_present
                                && !new_import.is_init_file_present
                            {
                                return false;
                            } else if !best_import_so_far.is_init_file_present
                                && new_import.is_init_file_present
                            {
                                return true;
                            }
                        }
                    }
                }

                // Prefer local over third-party. We check local first, so we should never
                // see the reverse.
                if best_import_so_far.import_type == ImportType::Local
                    && new_import.import_type == ImportType::ThirdParty
                {
                    return false;
                }

                // Prefer py.typed over non-py.typed.
                if best_import_so_far.py_typed_info.is_some() && new_import.py_typed_info.is_none()
                {
                    return false;
                } else if best_import_so_far.py_typed_info.is_none()
                    && new_import.py_typed_info.is_some()
                {
                    if best_import_so_far.import_type == new_import.import_type {
                        return true;
                    }
                }

                // Prefer pyi over py.
                if best_import_so_far.is_stub_file && !new_import.is_stub_file {
                    return false;
                } else if !best_import_so_far.is_stub_file && new_import.is_stub_file {
                    return true;
                }

                // All else equal, prefer shorter resolution paths.
                if best_import_so_far.resolved_uris.len() > new_import.resolved_uris.len() {
                    return true;
                }
            } else if new_import.is_partly_resolved {
                // If the new import is a traditional package but only partly resolves
                // the import but the best import so far is a namespace package, we need
                // to consider whether the best import so far also resolves the first part
                // of the import with a traditional package. Using the example "import a.b.c.d"
                // and the symbol ~ to represent a namespace package, consider the following
                // cases:
                //  bestSoFar: a/~b/~c/~d   new: a      Result: bestSoFar wins
                //  bestSoFar: ~a/~b/~c/~d  new: a      Result: new wins
                //  bestSoFar: a/~b/~c/~d   new: a/b    Result: new wins
                let so_far_index = first_non_empty(&best_import_so_far);
                let new_index = first_non_empty(&new_import);

                if so_far_index != new_index {
                    if so_far_index < 0 {
                        return true;
                    } else if new_index < 0 {
                        return false;
                    }
                    return so_far_index >= new_index;
                }
            }

            false
        })();

        Some(if pick_new_import {
            new_import
        } else {
            best_import_so_far
        })
    }

    // analyzer/importResolver.ts:1873
    fn _find_typeshed_path(
        &self,
        exec_env: &ExecutionEnvironment,
        module_descriptor: &ImportedModuleDescriptor,
        import_name: &str,
        is_std_lib: bool,
        import_logger: Option<&ImportLogger>,
    ) -> Option<ImportResult> {
        if let Some(logger) = import_logger {
            logger.log(&format!(
                "Looking for typeshed {} path",
                if is_std_lib {
                    python_path_utils::STD_LIB_FOLDER_NAME
                } else {
                    python_path_utils::THIRD_PARTY_FOLDER_NAME
                }
            ));
        }

        let mut typeshed_paths: Option<Vec<Uri>> = None;
        if is_std_lib {
            let path = self._get_stdlib_typeshed_path(
                self._config_options.get().typeshed_path,
                exec_env.python_version,
                exec_env.python_platform.as_deref(),
                import_logger,
                Some(module_descriptor),
            );

            if let Some(path) = path {
                typeshed_paths = Some(vec![path]);
            }
        } else {
            typeshed_paths = self._get_third_party_typeshed_package_paths(
                module_descriptor,
                import_logger,
                true,
            );
        }

        if let Some(typeshed_paths) = typeshed_paths {
            for typeshed_path in typeshed_paths {
                if self.dir_exists_cached(typeshed_path) {
                    let import_info = self.resolve_absolute_import(
                        None,
                        typeshed_path,
                        exec_env,
                        module_descriptor,
                        import_name,
                        import_logger,
                        false,
                        false,
                        false,
                        true,
                        false,
                    );

                    if let Some(mut import_info) = import_info
                        && import_info.is_import_found
                    {
                        let mut import_type = if is_std_lib {
                            ImportType::BuiltIn
                        } else {
                            ImportType::ThirdParty
                        };

                        // Handle 'typing_extensions' as a special case because it's
                        // part of stdlib typeshed stubs, but it's not part of stdlib.
                        if import_name == "typing_extensions" {
                            import_type = ImportType::ThirdParty;
                        }

                        import_info.import_type = import_type;
                        return Some(import_info);
                    }
                }
            }
        }

        if let Some(logger) = import_logger {
            logger.log("Typeshed path not found");
        }
        None
    }

    // analyzer/importResolver.ts:2051
    // Returns the directory for a module within the stdlib typeshed directory.
    // If moduleDescriptor is provided, it is filtered based on the VERSIONS
    // file in the typeshed stubs.
    fn _get_stdlib_typeshed_path(
        &self,
        custom_typeshed_path: Option<Uri>,
        python_version: PythonVersion,
        python_platform: Option<&str>,
        import_logger: Option<&ImportLogger>,
        module_descriptor: Option<&ImportedModuleDescriptor>,
    ) -> Option<Uri> {
        let subdirectory = self
            ._get_typeshed_info_provider()
            .get_typeshed_subdirectory(
                /* isStdLib */ true,
                custom_typeshed_path,
                import_logger,
            );
        if subdirectory.is_some()
            && let Some(module_descriptor) = module_descriptor
            && !self._is_stdlib_typeshed_stub_valid_for_version(
                module_descriptor,
                custom_typeshed_path,
                python_version,
                python_platform,
                import_logger,
            )
        {
            return None;
        }

        subdirectory
    }

    // analyzer/importResolver.ts:2080
    fn _get_third_party_typeshed_path(
        &self,
        custom_typeshed_path: Option<Uri>,
        import_logger: Option<&ImportLogger>,
    ) -> Option<Uri> {
        self._get_typeshed_info_provider()
            .get_typeshed_subdirectory(
                /* isStdLib */ false,
                custom_typeshed_path,
                import_logger,
            )
    }

    // analyzer/importResolver.ts:2088
    fn _is_stdlib_typeshed_stub_valid_for_version(
        &self,
        module_descriptor: &ImportedModuleDescriptor,
        custom_typeshed_path: Option<Uri>,
        python_version: PythonVersion,
        python_platform: Option<&str>,
        import_logger: Option<&ImportLogger>,
    ) -> bool {
        let versions = self
            ._get_typeshed_info_provider()
            .get_std_lib_module_version_info(custom_typeshed_path, import_logger)
            .get();

        // Loop through the name parts to make sure the module and submodules
        // referenced in the import statement are valid for this version of Python.
        for name_part_count in 1..=module_descriptor.name_parts.len() {
            let name_parts_to_consider = &module_descriptor.name_parts[..name_part_count];
            let version_info = versions.get(&name_parts_to_consider.join("."));

            if let Some(version_info) = version_info {
                if PythonVersion::is_less_than(&python_version, &version_info.min) {
                    return false;
                }

                if let Some(max) = &version_info.max {
                    if PythonVersion::is_greater_than(&python_version, max) {
                        return false;
                    }
                }

                if let Some(python_platform) = python_platform {
                    let python_platform_lower = python_platform.js_to_lower_case();

                    if let Some(supported_platforms) = &version_info.supported_platforms {
                        if supported_platforms
                            .iter()
                            .all(|p| p.js_to_lower_case() != python_platform_lower)
                        {
                            return false;
                        }
                    }

                    if let Some(unsupported_platforms) = &version_info.unsupported_platforms {
                        if unsupported_platforms
                            .iter()
                            .any(|p| p.js_to_lower_case() == python_platform_lower)
                        {
                            return false;
                        }
                    }
                }
            }
        }

        true
    }

    // analyzer/importResolver.ts:2133
    fn _get_third_party_typeshed_package_paths(
        &self,
        module_descriptor: &ImportedModuleDescriptor,
        import_logger: Option<&ImportLogger>,
        include_match_only: bool,
    ) -> Option<Vec<Uri>> {
        let package_map = self
            ._get_typeshed_info_provider()
            .get_third_party_package_map(self._config_options.get().typeshed_path, import_logger);
        let package_paths = &package_map.get().0;

        let first_name_part = if !module_descriptor.name_parts.is_empty() {
            module_descriptor.name_parts[0].as_str()
        } else {
            ""
        };
        if include_match_only {
            return package_paths.get(first_name_part).cloned();
        }

        if !first_name_part.is_empty() {
            return Some(
                get_map_values(package_paths, |k, _| k.starts_with(first_name_part))
                    .into_iter()
                    .flatten()
                    .collect(),
            );
        }

        Some(Vec::new())
    }

    // analyzer/importResolver.ts:2155
    fn _get_third_party_typeshed_package_roots(
        &self,
        import_logger: Option<&ImportLogger>,
    ) -> Vec<Uri> {
        let package_map = self
            ._get_typeshed_info_provider()
            .get_third_party_package_map(self._config_options.get().typeshed_path, import_logger);
        package_map.get().1.clone()
    }

    // analyzer/importResolver.ts:2163
    fn _get_typeshed_info_provider(&self) -> &'static TypeshedInfoProvider {
        self._typeshed_info_provider
    }

    // analyzer/importResolver.ts:2167
    fn _resolve_relative_import(
        &self,
        source_file_uri: Uri,
        exec_env: &ExecutionEnvironment,
        module_descriptor: &ImportedModuleDescriptor,
        import_name: &str,
        import_logger: Option<&ImportLogger>,
    ) -> Option<ImportResult> {
        if let Some(logger) = import_logger {
            logger.log("Attempting to resolve relative import");
        }

        // Determine which search path this file is part of.
        let directory = get_directory_leading_dots_points_to(
            source_file_uri.get_directory(),
            module_descriptor.leading_dots,
        );
        let Some(directory) = directory else {
            if let Some(logger) = import_logger {
                logger.log(&format!("Invalid relative path '{import_name}'"));
            }
            return None;
        };

        // Now try to match the module parts from the current directory location.
        let mut abs_import = self.resolve_absolute_import(
            Some(source_file_uri),
            directory,
            exec_env,
            module_descriptor,
            import_name,
            import_logger,
            /* allowPartial */ false,
            /* allowNativeLib */ true,
            /* useStubPackage */ false,
            /* allowPyi */ true,
            /* lookForPyTyped */ false,
        );

        if let Some(abs_import) = abs_import.as_mut()
            && abs_import.is_stub_file
        {
            // If we found a stub for a relative import, only search
            // the same folder for the real module. Otherwise, it will
            // error out on runtime.
            abs_import.non_stub_import_result = Some(P::new(
                self.resolve_absolute_import(
                    Some(source_file_uri),
                    directory,
                    exec_env,
                    module_descriptor,
                    import_name,
                    import_logger,
                    /* allowPartial */ false,
                    /* allowNativeLib */ true,
                    /* useStubPackage */ false,
                    /* allowPyi */ false,
                    /* lookForPyTyped */ false,
                )
                .unwrap_or_else(|| ImportResult {
                    import_name: import_name.to_string(),
                    is_relative: true,
                    is_import_found: false,
                    is_partly_resolved: false,
                    is_namespace_package: false,
                    is_stub_package: false,
                    is_init_file_present: false,
                    resolved_uris: Vec::new(),
                    import_type: ImportType::Local,
                    is_stub_file: false,
                    is_native_lib: false,
                    implicit_imports: None,
                    filtered_implicit_imports: None,
                    non_stub_import_result: None,
                    import_failure_info: None,
                    search_path: None,
                    is_stdlib_typeshed_file: None,
                    is_third_party_typeshed_file: None,
                    is_local_typings_file: None,
                    py_typed_info: None,
                    package_directory: None,
                }),
            ));
        }

        abs_import
    }

    // analyzer/importResolver.ts:2451
    // Retrieves the pytyped info for a directory if it exists. This is a small perf optimization
    // that allows skipping the search when the pytyped file doesn't exist.
    fn _get_py_typed_info(&self, file_path: Uri) -> Option<PyTypedInfo> {
        if !self.file_exists_cached(file_path.pytyped_uri()) {
            return None;
        }

        Some(get_py_typed_info_for_py_typed_file(
            self.file_system(),
            file_path.pytyped_uri(),
        ))
    }

    // analyzer/importResolver.ts:2459
    fn _find_and_resolve_native_module(
        &self,
        file_directory: Uri,
        dir_path: Uri,
        exec_env: &ExecutionEnvironment,
        import_name: &str,
        module_descriptor: &ImportedModuleDescriptor,
        import_logger: Option<&ImportLogger>,
        resolved_paths: &mut Vec<Uri>,
    ) -> bool {
        let mut is_native_lib = false;

        if !exec_env.skip_native_libraries && self.dir_exists_cached(file_directory) {
            let files_in_dir = self
                ._file_system_cache
                .get_files_in_directory(file_directory);
            let dir_name = dir_path.file_name();
            let native_lib_path = files_in_dir
                .iter()
                .copied()
                .find(|f| self._is_native_module_file_name(dir_name, *f));

            if let Some(native_lib_path) = native_lib_path {
                // Try resolving native library to a custom stub.
                is_native_lib = self._resolve_native_module_with_stub(
                    native_lib_path,
                    exec_env,
                    import_name,
                    module_descriptor,
                    import_logger,
                    resolved_paths,
                );

                if is_native_lib {
                    if let Some(logger) = import_logger {
                        logger.log(&format!(
                            "Resolved with native lib '{}'",
                            native_lib_path.to_user_visible_string()
                        ));
                    }
                }
            }
        }

        is_native_lib
    }

    // analyzer/importResolver.ts:2495
    fn _resolve_native_module_with_stub(
        &self,
        native_lib_path: Uri,
        exec_env: &ExecutionEnvironment,
        import_name: &str,
        module_descriptor: &ImportedModuleDescriptor,
        import_logger: Option<&ImportLogger>,
        resolved_paths: &mut Vec<Uri>,
    ) -> bool {
        let mut module_full_name = import_name.to_string();

        if module_descriptor.leading_dots > 0 {
            // Relative path. Convert `.mtrand` to `numpy.random.mtrand` based on search path.
            let info = self.get_module_name_for_import(native_lib_path, exec_env, false, false);
            module_full_name = if !info.module_name.is_empty() {
                info.module_name
            } else {
                module_full_name
            };
        }

        let compiled_stub_path =
            self.resolve_native_import_ex(native_lib_path, &module_full_name, import_logger);
        if let Some(compiled_stub_path) = compiled_stub_path {
            if let Some(logger) = import_logger {
                logger.log(&format!(
                    "Resolved native import {import_name} with stub '{}'",
                    compiled_stub_path.to_uri_string()
                ));
            }
            resolved_paths.push(compiled_stub_path);
            return false; // Resolved to a stub.
        }

        if let Some(logger) = import_logger {
            logger.log(&format!(
                "Resolved import with file '{}'",
                native_lib_path.to_uri_string()
            ));
        }
        resolved_paths.push(native_lib_path);
        true
    }

    // analyzer/importResolver.ts:2523
    fn _is_native_module_file_name(&self, module_name: &str, file_uri: Uri) -> bool {
        // Strip off the final file extension and the part of the file name
        // that excludes all (multi-part) file extensions. This allows us to
        // handle file names like "foo.cpython-32m.so".
        let file_extension = file_uri.last_extension().js_to_lower_case();
        let without_extension =
            strip_file_extension(file_uri.file_name(), /* multiDotExtension */ true);
        _is_native_module_file_extension(&file_extension)
            && equate_strings_case_insensitive(module_name, &without_extension)
    }

    // analyzer/importResolver.ts:2534
    fn _try_walk_up(&self, current: Option<Uri>) -> Option<Uri> {
        let current = current?;
        if current.is_empty() || current.is_root() {
            return None;
        }

        // Ensure we don't go around forever even if isRoot returns false.
        let next = current.resolve_paths(&[".."]);
        if next.equals(Some(current)) {
            return None;
        }
        Some(next)
    }

    // analyzer/importResolver.ts:2547
    fn _should_walk_up(
        &self,
        current: Option<Uri>,
        root: Uri,
        exec_env: &ExecutionEnvironment,
    ) -> bool {
        current.is_some_and(|current| {
            !current.is_empty()
                && (current.is_child(root)
                    || (current.equals(Some(root)) && is_default_workspace(exec_env.root)))
        })
    }
}

// analyzer/importResolver.ts:2556
pub type ImportResolverFactory =
    fn(ServiceProvider, P<ConfigOptions>, &'static dyn Host) -> ImportResolver;

// analyzer/importResolver.ts:2562
pub fn format_import_name(module_descriptor: &ImportedModuleDescriptor) -> String {
    ".".repeat(module_descriptor.leading_dots.max(0) as usize)
        + &module_descriptor.name_parts.join(".")
}

// analyzer/importResolver.ts:2566
pub fn get_parent_import_resolution_root(source_file_uri: Uri, execution_root: Option<Uri>) -> Uri {
    if !is_default_workspace(execution_root) {
        return execution_root.unwrap();
    }

    source_file_uri.get_directory()
}

// analyzer/importResolver.ts:2574
pub fn get_module_name_from_path(
    container_path: Uri,
    file_uri: Uri,
    strip_top_container_dir: bool,
) -> Option<String> {
    let module_name_info =
        _get_module_name_info_from_path(container_path, file_uri, strip_top_container_dir);
    let module_name_info = module_name_info?;
    if module_name_info.contains_invalid_characters == Some(true) {
        return None;
    }

    Some(module_name_info.module_name)
}

// analyzer/importResolver.ts:2587
fn _get_module_name_info_from_path(
    container_path: Uri,
    file_uri: Uri,
    strip_top_container_dir: bool,
) -> Option<ModuleNameInfoFromPath> {
    if !file_uri.starts_with(Some(container_path)) {
        return None;
    }

    let mut parts = container_path.get_relative_path_components(file_uri);
    if !parts.is_empty() {
        let orig_last_part = parts[parts.len() - 1].clone();

        // Strip the file extension from the last part.
        let mut new_last_part = strip_file_extension(&orig_last_part, false);

        // If module is native, strip platform part, such as 'cp36-win_amd64' in 'mtrand.cp36-win_amd64'.
        if _is_native_module_file_extension(&get_file_extension(&orig_last_part, false)) {
            new_last_part = strip_file_extension(&new_last_part, false);
        }

        let last = parts.len() - 1;
        parts[last] = new_last_part.clone();

        // Strip off the '/__init__' if it's present.
        if new_last_part == "__init__" {
            parts.pop();
        }
    }

    if strip_top_container_dir {
        if parts.is_empty() {
            return None;
        }
        parts.remove(0);
    }

    if parts.is_empty() {
        return None;
    }

    // Handle the case where the symbol was resolved to a stubs package
    // rather than the real package. We'll strip off the "-stubs" suffix
    // in this case.
    if parts[0].ends_with(STUBS_SUFFIX) {
        let len = parts[0].len() - STUBS_SUFFIX.len();
        parts[0].truncate(len);
    }

    // Check whether parts contains invalid characters.
    let contains_invalid_characters = parts
        .iter()
        .any(|p| !Tokenizer::is_python_identifier(&p.encode_utf16().collect::<Vec<u16>>()));

    Some(ModuleNameInfoFromPath {
        module_name: parts.join("."),
        contains_invalid_characters: Some(contains_invalid_characters),
    })
}

// analyzer/importResolver.ts:2643
fn _is_native_module_file_extension(file_extension: &str) -> bool {
    SUPPORTED_NATIVE_LIB_EXTENSIONS.contains(&file_extension)
}

// analyzer/importResolver.ts:2647
pub fn is_default_workspace(uri: Option<Uri>) -> bool {
    match uri {
        None => true,
        Some(uri) => uri.is_empty() || Uri::is_default_workspace(uri),
    }
}

#[cfg(test)]
mod tests {
    //! Port of `tests/importResolver.test.ts`, then tests of paths it does not cover.

    use super::*;
    use crate::common::case_sensitivity_detector::CaseSensitiveDetector;
    use crate::common::console::NullConsole;
    use crate::common::path_consts::{LIB, SITE_PACKAGES, TYPESHED_FALLBACK};
    use crate::common::path_utils::{combine_paths, get_directory_path, normalize_slashes};
    use crate::common::python_version::{PYTHON_VERSION3_9, PYTHON_VERSION3_13};
    use crate::partial_stub_service::PartialStubService;
    use crate::pyright_file_system::PyrightFileSystem;
    use crate::tests::harness::test_access_host::TestAccessHost;
    use crate::tests::harness::vfs::filesystem::{TestFileSystem, file};

    // tests/importResolver.test.ts:31
    fn library_root() -> String {
        combine_paths(&normalize_slashes("/"), &[LIB, SITE_PACKAGES])
    }

    fn lib(parts: &[&str]) -> String {
        combine_paths(&library_root(), parts)
    }

    fn path(parts: &[&str]) -> String {
        combine_paths("/", parts)
    }

    struct TestFile {
        path: String,
        content: &'static str,
    }

    fn tf(path: String, content: &'static str) -> TestFile {
        TestFile { path, content }
    }

    // tests/importResolver.test.ts:1088
    fn create_test_file_system(files: &[TestFile]) -> &'static TestFileSystem {
        let fs = TestFileSystem::leak();

        for file in files {
            let path = normalize_slashes(&file.path);
            let dir = get_directory_path(&path);
            fs.mkdirp_sync(&dir);

            fs.write_file_sync(&path, file.content);
        }

        fs
    }

    /// `createServiceProvider(testFS, fs, new PartialStubService(fs))` with
    /// `fs = new PyrightFileSystem(testFS)`.
    fn create_service_provider_for(test_fs: &'static TestFileSystem) -> ServiceProvider {
        let fs: &'static PyrightFileSystem = Box::leak(Box::new(PyrightFileSystem::new(test_fs)));
        let partial_stub_service: &'static PartialStubService =
            Box::leak(Box::new(PartialStubService::new(fs)));
        let console: &'static NullConsole = Box::leak(Box::default());
        let sp = ServiceProvider::new(fs, console, &CaseSensitiveDetector);
        assert!(sp.partial_stubs.set(partial_stub_service).is_ok());
        sp
    }

    // tests/importResolver.test.ts:1102
    fn create_service_provider_from_files(files: &[TestFile]) -> ServiceProvider {
        create_service_provider_for(create_test_file_system(files))
    }

    fn descriptor(name_parts: &[&str]) -> ImportedModuleDescriptor {
        ImportedModuleDescriptor {
            leading_dots: 0,
            name_parts: name_parts.iter().map(|s| s.to_string()).collect(),
            has_trailing_dot: None,
            imported_symbols: Some(JsSet::default()),
        }
    }

    struct Setup {
        import_resolver: ImportResolver,
        uri: Uri,
        config_options: P<ConfigOptions>,
        test_fs: &'static TestFileSystem,
    }

    impl Setup {
        fn resolve(&self, name_parts: &[&str]) -> P<ImportResult> {
            self.resolve_with(&descriptor(name_parts))
        }

        fn resolve_with(&self, md: &ImportedModuleDescriptor) -> P<ImportResult> {
            self.import_resolver.resolve_import(
                self.uri,
                &self.config_options.find_exec_environment(self.uri),
                md,
            )
        }
    }

    // tests/importResolver.test.ts:1032
    fn setup_import_resolver(files: &[TestFile], setup: impl FnOnce(&mut ConfigOptions)) -> Setup {
        let test_fs = create_test_file_system(files);
        let sp = create_service_provider_for(test_fs);
        let mut config_options = ConfigOptions::new(file("/"));
        setup(&mut config_options);

        let file_path = files
            .last()
            .map_or_else(|| combine_paths("src", &["file.py"]), |f| f.path.clone());

        let uri = file(&file_path);
        let host =
            TestAccessHost::leak(sp.fs().get_module_path(), vec![file(&library_root())], None);
        let import_resolver = ImportResolver::new(sp, P::new(config_options), host);

        Setup {
            config_options: import_resolver.get_config_options(),
            import_resolver,
            uri,
            test_fs,
        }
    }

    // tests/importResolver.test.ts:995
    fn get_import_result(
        files: &[TestFile],
        name_parts: &[&str],
        setup: impl FnOnce(&mut ConfigOptions),
    ) -> P<ImportResult> {
        setup_import_resolver(files, setup).resolve(name_parts)
    }

    // tests/importResolver.test.ts:1018
    fn get_module_name_for_import(files: &[TestFile]) -> ModuleImportInfo {
        let s = setup_import_resolver(files, |_| {});

        s.import_resolver.get_module_name_for_import(
            s.uri,
            &s.config_options.find_exec_environment(s.uri),
            false,
            /* detectPyTyped */ true,
        )
    }

    fn count_resolved(import_result: &ImportResult, path: &str) -> usize {
        import_result
            .resolved_uris
            .iter()
            .filter(|f| !f.is_empty() && f.get_file_path() == path)
            .count()
    }

    fn last_resolved(import_result: &ImportResult) -> &'static str {
        import_result.resolved_uris[import_result.resolved_uris.len() - 1].get_file_path()
    }

    // Import tests that cannot run in a true venv.

    #[test]
    fn partial_stub_file_exists() {
        let files = [
            tf(lib(&["myLib-stubs", "partialStub.pyi"]), "def test(): ..."),
            tf(lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            tf(lib(&["myLib", "partialStub.py"]), "def test(): pass"),
        ];

        let import_result = get_import_result(&files, &["myLib", "partialStub"], |_| {});
        assert!(import_result.is_import_found);
        assert!(import_result.is_stub_file);
        assert_eq!(
            1,
            count_resolved(&import_result, &lib(&["myLib", "partialStub.pyi"]))
        );
    }

    #[test]
    fn symlinked_partial_stub_file_exists() {
        let stub_source = combine_paths(&normalize_slashes("/"), &["wheel", "partialStub.pyi"]);
        let files = [
            tf(stub_source.clone(), "def test(): ..."),
            tf(lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            tf(lib(&["myLib", "partialStub.py"]), "def test(): pass"),
        ];

        let test_fs = create_test_file_system(&files);
        test_fs.symlink_sync(&stub_source, &lib(&["myLib-stubs", "partialStub.pyi"]));
        let sp = create_service_provider_for(test_fs);
        let config_options = P::new(ConfigOptions::new(file("/")));
        let uri = file(&files[files.len() - 1].path);
        let import_resolver = ImportResolver::new(
            sp,
            config_options,
            TestAccessHost::leak(sp.fs().get_module_path(), vec![file(&library_root())], None),
        );

        let import_result = import_resolver.resolve_import(
            uri,
            &config_options.find_exec_environment(uri),
            &descriptor(&["myLib", "partialStub"]),
        );

        assert!(import_result.is_import_found);
        assert!(import_result.is_stub_file);
        assert_eq!(
            1,
            count_resolved(&import_result, &lib(&["myLib", "partialStub.pyi"]))
        );
    }

    #[test]
    fn partial_stub_init_exists() {
        let files = [
            tf(lib(&["myLib-stubs", "__init__.pyi"]), "def test(): ..."),
            tf(lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            tf(lib(&["myLib", "__init__.py"]), "def test(): pass"),
        ];

        let import_result = get_import_result(&files, &["myLib"], |_| {});
        assert!(import_result.is_import_found);
        assert!(import_result.is_stub_file);
        assert_eq!(
            1,
            count_resolved(&import_result, &lib(&["myLib", "__init__.pyi"]))
        );
    }

    #[test]
    fn stub_package() {
        let files = [
            tf(lib(&["myLib-stubs", "stub.pyi"]), "# empty"),
            tf(lib(&["myLib-stubs", "__init__.pyi"]), "# empty"),
            tf(lib(&["myLib", "partialStub.py"]), "def test(): pass"),
        ];

        // If fully typed stub package exists, that wins over the real package.
        let import_result = get_import_result(&files, &["myLib", "partialStub"], |_| {});
        assert!(!import_result.is_import_found);
    }

    #[test]
    fn partial_stub_package_in_typing_folder() {
        let typing_folder = combine_paths(&normalize_slashes("/"), &["typing"]);
        let files = [
            tf(
                combine_paths(&typing_folder, &["myLib-stubs", "__init__.pyi"]),
                "def test(): ...",
            ),
            tf(
                combine_paths(&typing_folder, &["myLib-stubs", "py.typed"]),
                "partial\n",
            ),
            tf(lib(&["myLib", "__init__.py"]), "def test(): pass"),
        ];

        let import_result = get_import_result(&files, &["myLib"], |c| {
            c.stub_path = Some(file(&typing_folder))
        });
        assert!(import_result.is_import_found);
        assert!(import_result.is_stub_file);
        assert_eq!(
            1,
            count_resolved(&import_result, &lib(&["myLib", "__init__.pyi"]))
        );
    }

    #[test]
    fn typeshed_folder() {
        let typeshed_folder = combine_paths(&normalize_slashes("/"), &["ts"]);
        let files = [
            tf(lib(&["myLib-stubs", "__init__.pyi"]), "def test(): ..."),
            tf(lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            tf(
                combine_paths(&typeshed_folder, &["stubs", "myLibPackage", "myLib.pyi"]),
                "# empty",
            ),
            tf(lib(&["myLib", "__init__.py"]), "def test(): pass"),
        ];

        // Stub packages win over typeshed.
        let import_result = get_import_result(&files, &["myLib"], |c| {
            c.typeshed_path = Some(file(&typeshed_folder))
        });
        assert!(import_result.is_import_found);
        assert!(import_result.is_stub_file);
        assert_eq!(
            1,
            count_resolved(&import_result, &lib(&["myLib", "__init__.pyi"]))
        );
    }

    #[test]
    fn typeshed_fallback_folder() {
        let files = [
            tf(lib(&["myLib-stubs", "__init__.pyi"]), "def test(): ..."),
            tf(lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            tf(
                path(&[TYPESHED_FALLBACK, "stubs", "myLibPackage", "myLib.pyi"]),
                "# empty",
            ),
            tf(lib(&["myLib", "__init__.py"]), "def test(): pass"),
        ];

        // Stub packages win over typeshed.
        let import_result = get_import_result(&files, &["myLib"], |_| {});
        assert!(import_result.is_import_found);
        assert!(import_result.is_stub_file);
        assert_eq!(
            1,
            count_resolved(&import_result, &lib(&["myLib", "__init__.pyi"]))
        );
    }

    #[test]
    #[ignore = "getSourceFilesFromStub serves the language server (SourceMapper) and is not ported"]
    fn get_source_files_from_stub_maps_stdlib_typeshed_stubs_into_python_search_paths() {}

    #[test]
    #[ignore = "getSourceFilesFromStub serves the language server (SourceMapper) and is not ported"]
    fn get_source_files_from_stub_preserves_stdlib_source_roots_with_configured_venv() {}

    #[test]
    fn configured_venv_site_packages_keep_precedence_over_preserved_stdlib_source_roots() {
        let stdlib_source_root = combine_paths(&normalize_slashes("/"), &["python-stdlib"]);
        let venv_path = combine_paths(&normalize_slashes("/"), &["venvs"]);
        let venv_name = "test";
        let venv_site_packages = combine_paths(&venv_path, &[venv_name, LIB, SITE_PACKAGES]);
        let venv_module_path = combine_paths(&venv_site_packages, &["shadowed.py"]);
        let files = [
            tf(
                combine_paths(&stdlib_source_root, &["shadowed.py"]),
                "x = \"stdlib\"",
            ),
            tf(venv_module_path.clone(), "x = \"venv\""),
            tf(path(&["src", "test.py"]), "import shadowed"),
        ];

        let sp = create_service_provider_from_files(&files);
        let mut config_options = ConfigOptions::new(file("/"));
        config_options.venv_path = Some(file(&venv_path));
        config_options.venv = Some(venv_name.to_string());
        config_options.python_path = Some(file(&combine_paths(&venv_path, &[venv_name, "python"])));
        let config_options = P::new(config_options);

        let import_resolver = ImportResolver::new(
            sp,
            config_options,
            TestAccessHost::leak(
                file(&combine_paths(&venv_path, &[venv_name])),
                vec![file(&stdlib_source_root), file(&venv_site_packages)],
                Some(sp.fs()),
            ),
        );
        let uri = file(&path(&["src", "test.py"]));
        let exec_env = config_options.find_exec_environment(uri);
        let import_result =
            import_resolver.resolve_import(uri, &exec_env, &descriptor(&["shadowed"]));

        assert!(import_result.is_import_found);
        let resolved: Vec<&str> = import_result
            .resolved_uris
            .iter()
            .map(|u| u.get_file_path())
            .collect();
        assert_eq!(resolved, [venv_module_path.as_str()]);
    }

    #[test]
    fn py_typed_file() {
        let files = [
            tf(lib(&["myLib-stubs", "__init__.pyi"]), "def test(): ..."),
            tf(lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            tf(lib(&["myLib", "__init__.py"]), "def test(): pass"),
            tf(lib(&["myLib", "py.typed"]), "# typed"),
        ];

        // Partial stub package always overrides original package.
        let import_result = get_import_result(&files, &["myLib"], |_| {});
        assert!(import_result.is_import_found);
        assert!(import_result.is_stub_file);
    }

    #[test]
    fn py_typed_library() {
        let files = [
            tf(lib(&["os", "__init__.py"]), "def test(): ..."),
            tf(lib(&["os", "py.typed"]), ""),
            tf(
                path(&[TYPESHED_FALLBACK, "stubs", "os", "os", "__init__.pyi"]),
                "# empty",
            ),
        ];

        let import_result = get_import_result(&files, &["os"], |_| {});
        assert!(import_result.is_import_found);
        assert_eq!(files[0].path, last_resolved(&import_result));
    }

    #[test]
    fn import_side_by_side_file_sub_under_lib_folder() {
        let files = [
            tf(
                combine_paths("/lib/site-packages/myLib", &["file1.py"]),
                "def test1(): ...",
            ),
            tf(
                combine_paths("/lib/site-packages/myLib", &["file2.py"]),
                "def test2(): ...",
            ),
        ];

        let import_result = get_import_result(&files, &["file1"], |_| {});
        assert!(!import_result.is_import_found);
    }

    #[test]
    fn get_module_name_for_import_library_file() {
        let files = [tf(lib(&["myLib", "myModule", "file1.py"]), "# empty")];

        let module_import_info = get_module_name_for_import(&files);

        assert_eq!(module_import_info.import_type, ImportType::ThirdParty);
        assert!(!module_import_info.is_third_party_py_typed_present);
        assert!(!module_import_info.is_local_typings_file);
    }

    #[test]
    fn get_module_name_for_import_py_typed_library_file() {
        let files = [
            tf(lib(&["myLib", "py.typed"]), ""),
            tf(lib(&["myLib", "myModule", "file1.py"]), "# empty"),
        ];

        let module_import_info = get_module_name_for_import(&files);

        assert_eq!(module_import_info.import_type, ImportType::ThirdParty);
        assert!(module_import_info.is_third_party_py_typed_present);
        assert!(!module_import_info.is_local_typings_file);
    }

    #[test]
    fn import_found_in_symlinked_file() {
        let files = [
            tf(path(&["external", "file2.py"]), "def f(): pass"),
            tf(path(&["src", "file1.py"]), "import file2"),
        ];

        let s = setup_import_resolver(&files, |c| {
            c.default_extra_paths = Some(vec![file(&path(&["external_symlinked"]))]);
        });
        s.test_fs.mkdir_sync(&path(&["external_symlinked"]));
        s.test_fs.symlink_sync(
            &path(&["external", "file2.py"]),
            &path(&["external_symlinked", "file2.py"]),
        );

        let result = s.resolve(&["file2"]);
        assert!(result.is_import_found);
    }

    // Import tests that can run with or without a true venv.

    #[test]
    fn side_by_side_files() {
        let my_file = combine_paths("src", &["file.py"]);
        let files = [
            tf(lib(&["myLib-stubs", "partialStub.pyi"]), "def test(): ..."),
            tf(lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            tf(lib(&["myLib", "partialStub.pyi"]), "# empty"),
            tf(lib(&["myLib", "partialStub.py"]), "def test(): pass"),
            tf(lib(&["myLib-stubs", "partialStub2.pyi"]), "def test(): ..."),
            tf(lib(&["myLib", "partialStub2.py"]), "def test(): pass"),
            tf(my_file.clone(), "# not used"),
        ];

        let sp = create_service_provider_from_files(&files);
        let config_options = P::new(ConfigOptions::new(file("/")));
        let import_resolver = ImportResolver::new(
            sp,
            config_options,
            TestAccessHost::leak(sp.fs().get_module_path(), vec![file(&library_root())], None),
        );

        // Stub package wins over original package (per PEP 561 rules).
        let my_uri = file(&my_file);
        let side_by_side_result = import_resolver.resolve_import(
            my_uri,
            &config_options.find_exec_environment(my_uri),
            &descriptor(&["myLib", "partialStub"]),
        );

        assert!(side_by_side_result.is_import_found);
        assert!(side_by_side_result.is_stub_file);

        let side_by_side_stub_file = file(&lib(&["myLib", "partialStub.pyi"]));
        assert_eq!(
            1,
            side_by_side_result
                .resolved_uris
                .iter()
                .filter(|f| f.key() == side_by_side_stub_file.key())
                .count()
        );
        assert_eq!(
            "def test(): ...",
            sp.fs().read_file_text_sync(side_by_side_stub_file).unwrap()
        );

        // Side by side stub doesn't completely disable partial stub.
        let partial_stub_result = import_resolver.resolve_import(
            my_uri,
            &config_options.find_exec_environment(my_uri),
            &descriptor(&["myLib", "partialStub2"]),
        );

        assert!(partial_stub_result.is_import_found);
        assert!(partial_stub_result.is_stub_file);

        let partial_stub_file = file(&lib(&["myLib", "partialStub2.pyi"]));
        assert_eq!(
            1,
            partial_stub_result
                .resolved_uris
                .iter()
                .filter(|f| f.key() == partial_stub_file.key())
                .count()
        );
    }

    #[test]
    fn stub_namespace_package() {
        let files = [
            tf(lib(&["myLib-stubs", "stub.pyi"]), "# empty"),
            tf(lib(&["myLib", "partialStub.py"]), "def test(): pass"),
        ];

        // If fully typed stub package exists, that wins over the real package.
        let import_result = get_import_result(&files, &["myLib", "partialStub"], |_| {});
        assert!(import_result.is_import_found);
        assert!(!import_result.is_stub_file);
        assert_eq!(
            1,
            count_resolved(&import_result, &lib(&["myLib", "partialStub.py"]))
        );
    }

    #[test]
    fn py_typed_namespace_package_plus_stubs() {
        let typing_folder = combine_paths(&normalize_slashes("/"), &["typing"]);
        let files = [
            tf(
                combine_paths(&typing_folder, &["myLib/core", "foo.pyi"]),
                "def test(): pass",
            ),
            tf(lib(&["myLib", "py.typed"]), ""),
            tf(lib(&["myLib", "__init__.py"]), "def test(): pass"),
            tf(lib(&["myLib", "__init__.pyi"]), "def test(): pass"),
        ];

        let import_result = get_import_result(&files, &["myLib"], |c| {
            c.stub_path = Some(file(&typing_folder))
        });
        assert!(import_result.is_import_found);
        assert!(import_result.is_stub_file);
        assert_eq!(
            1,
            count_resolved(&import_result, &lib(&["myLib", "__init__.pyi"]))
        );
    }

    #[test]
    fn stub_in_typing_folder_over_partial_stub_package() {
        let typing_folder = combine_paths(&normalize_slashes("/"), &["typing"]);
        let files = [
            tf(lib(&["myLib-stubs", "__init__.pyi"]), "def test(): ..."),
            tf(lib(&["myLib-stubs", "py.typed"]), "partial\n"),
            tf(combine_paths(&typing_folder, &["myLib.pyi"]), "# empty"),
            tf(lib(&["myLib", "__init__.py"]), "def test(): pass"),
        ];

        // If the package exists in typing folder, that gets picked up first.
        let import_result = get_import_result(&files, &["myLib"], |c| {
            c.stub_path = Some(file(&typing_folder))
        });
        assert!(import_result.is_import_found);
        assert!(import_result.is_stub_file);
        assert_eq!(
            0,
            count_resolved(&import_result, &lib(&["myLib", "__init__.pyi"]))
        );
    }

    #[test]
    fn non_py_typed_library() {
        let files = [
            tf(lib(&["os", "__init__.py"]), "def test(): ..."),
            tf(
                path(&[TYPESHED_FALLBACK, "stubs", "os", "os", "__init__.pyi"]),
                "# empty",
            ),
        ];

        let import_result = get_import_result(&files, &["os"], |_| {});
        assert!(import_result.is_import_found);
        assert_eq!(files[1].path, last_resolved(&import_result));
    }

    #[test]
    fn no_empty_import_roots() {
        let sp = create_service_provider_from_files(&[]);
        let config_options = ConfigOptions::new(Uri::empty()); // Empty, like open-file mode.
        let import_resolver = ImportResolver::new(
            sp,
            P::new(config_options.clone()),
            TestAccessHost::leak(sp.fs().get_module_path(), vec![file(&library_root())], None),
        );
        // `forEach((path) => assert(path))`: every root is a Uri, never undefined.
        let roots =
            import_resolver.get_import_roots(&config_options.get_default_exec_environment(), false);
        assert!(!roots.is_empty());
    }

    #[test]
    fn multiple_typeshed_fallback() {
        let files = [
            tf(
                path(&[TYPESHED_FALLBACK, "stubs", "aLib", "aLib", "__init__.pyi"]),
                "# empty",
            ),
            tf(
                path(&[TYPESHED_FALLBACK, "stubs", "bLib", "bLib", "__init__.pyi"]),
                "# empty",
            ),
        ];

        let sp = create_service_provider_from_files(&files);
        let config_options = ConfigOptions::new(Uri::empty()); // Empty, like open-file mode.
        let import_resolver = ImportResolver::new(
            sp,
            P::new(config_options.clone()),
            TestAccessHost::leak(sp.fs().get_module_path(), vec![file(&library_root())], None),
        );
        let import_roots =
            import_resolver.get_import_roots(&config_options.get_default_exec_environment(), false);

        let count = |p: &str| {
            import_roots
                .iter()
                .filter(|f| !f.is_empty() && f.get_file_path() == p)
                .count()
        };
        assert_eq!(1, count(&path(&[TYPESHED_FALLBACK, "stubs", "aLib"])));
        assert_eq!(1, count(&path(&[TYPESHED_FALLBACK, "stubs", "bLib"])));
    }

    #[test]
    fn import_side_by_side_file_root() {
        let files = [
            tf(path(&["file1.py"]), "def test1(): ..."),
            tf(path(&["file2.py"]), "def test2(): ..."),
        ];

        let import_result = get_import_result(&files, &["file1"], |_| {});
        assert!(import_result.is_import_found);
        assert_eq!(1, count_resolved(&import_result, &path(&["file1.py"])));
    }

    #[test]
    fn import_side_by_side_file_sub_folder() {
        let files = [
            tf(combine_paths("/test", &["file1.py"]), "def test1(): ..."),
            tf(combine_paths("/test", &["file2.py"]), "def test2(): ..."),
        ];

        let import_result = get_import_result(&files, &["file1"], |_| {});
        assert!(import_result.is_import_found);
        assert_eq!(
            1,
            count_resolved(&import_result, &combine_paths("/test", &["file1.py"]))
        );
    }

    #[test]
    fn import_side_by_side_file_sub_under_src_folder() {
        let files = [
            tf(
                combine_paths("/src/nested", &["file1.py"]),
                "def test1(): ...",
            ),
            tf(
                combine_paths("/src/nested", &["file2.py"]),
                "def test2(): ...",
            ),
        ];

        let import_result = get_import_result(&files, &["file1"], |_| {});
        assert!(import_result.is_import_found);
        assert_eq!(
            1,
            count_resolved(&import_result, &combine_paths("/src/nested", &["file1.py"]))
        );
    }

    #[test]
    fn import_file_sub_under_containing_folder() {
        let files = [
            tf(
                combine_paths("/src/nested", &["file1.py"]),
                "def test1(): ...",
            ),
            tf(
                combine_paths("/src/nested/nested2", &["file2.py"]),
                "def test2(): ...",
            ),
        ];

        let import_result = get_import_result(&files, &["file1"], |_| {});
        assert!(import_result.is_import_found);
        assert_eq!(
            1,
            count_resolved(&import_result, &combine_paths("/src/nested", &["file1.py"]))
        );
    }

    #[test]
    fn dont_walk_up_the_root() {
        let files = [tf(path(&["file1.py"]), "def test1(): ...")];

        let import_result =
            get_import_result(&files, &["notExist"], |c| c.project_root = Uri::empty());
        assert!(!import_result.is_import_found);
    }

    fn two_package_roots(c: &mut ConfigOptions) {
        c.default_extra_paths = Some(vec![
            file(&path(&["packages1"])),
            file(&path(&["packages2"])),
        ]);
    }

    #[test]
    fn nested_namespace_package_1() {
        let files = [
            tf(path(&["packages1", "a", "b", "c", "d.py"]), "def f(): pass"),
            tf(path(&["packages1", "a", "__init__.py"]), ""),
            tf(path(&["packages2", "a", "__init__.py"]), ""),
        ];

        let import_result = get_import_result(&files, &["a", "b", "c", "d"], two_package_roots);
        assert!(import_result.is_import_found);
    }

    #[test]
    fn nested_namespace_package_2() {
        let files = [
            tf(path(&["packages1", "a", "b", "c", "d.py"]), "def f(): pass"),
            tf(path(&["packages1", "a", "b", "c", "__init__.py"]), ""),
            tf(path(&["packages2", "a", "b", "c", "__init__.py"]), ""),
        ];

        let import_result = get_import_result(&files, &["a", "b", "c", "d"], two_package_roots);
        assert!(import_result.is_import_found);
    }

    #[test]
    fn nested_namespace_package_3() {
        let files = [
            tf(path(&["packages1", "a", "b", "c", "d.py"]), "def f(): pass"),
            tf(path(&["packages2", "a", "__init__.py"]), ""),
        ];

        let import_result = get_import_result(&files, &["a", "b", "c", "d"], two_package_roots);
        assert!(!import_result.is_import_found);
    }

    #[test]
    fn nested_namespace_package_4() {
        let files = [
            tf(path(&["packages1", "a", "b", "__init__.py"]), ""),
            tf(path(&["packages1", "a", "b", "c.py"]), "def f(): pass"),
            tf(path(&["packages2", "a", "__init__.py"]), ""),
            tf(path(&["packages2", "a", "b", "__init__.py"]), ""),
        ];

        let import_result = get_import_result(&files, &["a", "b", "c"], two_package_roots);
        assert!(!import_result.is_import_found);
    }

    #[test]
    fn default_workspace_importing_side_by_side_file() {
        let files = [
            tf(path(&["src", "a", "b", "file1.py"]), "import file2"),
            tf(path(&["src", "a", "b", "file2.py"]), "def f(): pass"),
        ];

        let import_result = get_import_result(&files, &["file2"], |config| {
            config.project_root = Uri::default_workspace(&CaseSensitiveDetector);
        });
        assert!(import_result.is_import_found);
    }

    #[test]
    fn get_module_name_for_import_user_file() {
        let files = [tf(path(&["src", "file1.py"]), "# empty")];

        let module_import_info = get_module_name_for_import(&files);

        assert_eq!(module_import_info.import_type, ImportType::Local);
        assert!(!module_import_info.is_third_party_py_typed_present);
        assert!(!module_import_info.is_local_typings_file);
    }

    // Import tests that have to run with a venv.

    #[test]
    #[ignore = "needs a real venv with pytest: set CI_IMPORT_TEST_VENVPATH and CI_IMPORT_TEST_VENV"]
    fn venv_can_find_imports() {
        use crate::common::real_file_system::{RealTempFile, create_from_real_file_system};

        let detector: &'static RealTempFile = Box::leak(Box::default());
        let console: &'static NullConsole = Box::leak(Box::default());
        let real_fs = Box::leak(Box::new(create_from_real_file_system(detector, console)));
        let fs: &'static PyrightFileSystem = Box::leak(Box::new(PyrightFileSystem::new(real_fs)));
        let sp = ServiceProvider::new(fs, console, detector);

        let mut config_options = ConfigOptions::new(file("/"));
        let venv_path = std::env::var("CI_IMPORT_TEST_VENVPATH").expect("CI_IMPORT_TEST_VENVPATH");
        config_options.venv_path = Some(Uri::file(&venv_path, detector, true));
        config_options.venv = std::env::var("CI_IMPORT_TEST_VENV").ok();
        let config_options = P::new(config_options);

        let uri = file(&path(&["file1.py"]));
        let import_resolver = ImportResolver::new(
            sp,
            config_options,
            TestAccessHost::leak(sp.fs().get_module_path(), vec![file(&library_root())], None),
        );
        let import_result = import_resolver.resolve_import(
            uri,
            &config_options.find_exec_environment(uri),
            &descriptor(&["pytest"]),
        );
        assert!(
            import_result.is_import_found,
            "Import not found: {:?}",
            import_result.import_failure_info
        );
    }

    // Paths `importResolver.test.ts` does not cover.

    #[test]
    fn relative_import_with_stub_resolves_non_stub_in_same_folder() {
        let files = [
            tf(path(&["proj", "pkg", "mod.pyi"]), ""),
            tf(path(&["proj", "pkg", "mod.py"]), ""),
            tf(path(&["proj", "pkg", "main.py"]), "from .mod import x"),
        ];
        let s = setup_import_resolver(&files, |c| c.project_root = file("/proj"));

        let result = s.resolve_with(&ImportedModuleDescriptor {
            leading_dots: 1,
            ..descriptor(&["mod"])
        });
        assert!(result.is_import_found);
        assert!(result.is_relative);
        assert!(result.is_stub_file);
        assert_eq!(last_resolved(&result), "/proj/pkg/mod.pyi");
        let non_stub = result.non_stub_import_result.expect("nonStubImportResult");
        assert!(non_stub.is_import_found);
        assert_eq!(last_resolved(&non_stub), "/proj/pkg/mod.py");

        // More dots than directories above the file fails.
        let result = s.resolve_with(&ImportedModuleDescriptor {
            leading_dots: 5,
            ..descriptor(&["mod"])
        });
        assert!(!result.is_import_found);
    }

    #[test]
    fn implicit_imports_prefer_stubs_and_follow_name_order() {
        let files = [
            tf(path(&["proj", "pkg", "__init__.py"]), ""),
            tf(path(&["proj", "pkg", "b.py"]), ""),
            tf(path(&["proj", "pkg", "a.pyi"]), ""),
            tf(path(&["proj", "pkg", "a.py"]), ""),
            tf(path(&["proj", "pkg", "sub", "__init__.py"]), ""),
            tf(path(&["proj", "pkg", "notes.txt"]), ""),
            tf(path(&["proj", "main.py"]), ""),
        ];
        let s = setup_import_resolver(&files, |c| c.project_root = file("/proj"));

        let result = s.resolve_with(&ImportedModuleDescriptor {
            imported_symbols: None,
            ..descriptor(&["pkg"])
        });
        assert!(result.is_import_found);
        let implicit_imports = result.implicit_imports.as_ref().unwrap();
        let names: Vec<&str> = implicit_imports.keys().map(String::as_str).collect();
        assert_eq!(names, ["a", "b", "sub"]);
        assert!(implicit_imports["a"].is_stub_file);
        assert_eq!(
            implicit_imports["sub"].uri.get_file_path(),
            "/proj/pkg/sub/__init__.py"
        );
        // `importedSymbols` undefined clears the filtered map.
        assert!(result.filtered_implicit_imports.is_none());

        let mut symbols = JsSet::default();
        symbols.insert("b".to_string());
        let result = s.resolve_with(&ImportedModuleDescriptor {
            imported_symbols: Some(symbols),
            ..descriptor(&["pkg"])
        });
        let filtered: Vec<&str> = result
            .filtered_implicit_imports
            .as_ref()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(filtered, ["b"]);
    }

    #[test]
    fn native_module_resolves_with_multi_dot_extension() {
        let files = [
            tf(lib(&["nat", "__init__.py"]), ""),
            tf(lib(&["nat", "fast.cpython-311-x86_64-linux-gnu.so"]), ""),
            tf(path(&["src", "main.py"]), ""),
        ];
        let s = setup_import_resolver(&files, |_| {});

        let result = s.resolve(&["nat", "fast"]);
        assert!(result.is_import_found);
        assert!(result.is_native_lib);
        assert_eq!(result.import_type, ImportType::ThirdParty);
        assert_eq!(
            last_resolved(&result),
            lib(&["nat", "fast.cpython-311-x86_64-linux-gnu.so"])
        );
    }

    fn typeshed_files() -> Vec<TestFile> {
        vec![
            tf(
                path(&["ts", "stdlib", "VERSIONS"]),
                "os: 3.0-\nnewmod: 3.12-\nbye: 3.0-3.10\nwinonly: 3.0- ; platforms=win32\n",
            ),
            tf(path(&["ts", "stdlib", "os", "__init__.pyi"]), ""),
            tf(path(&["ts", "stdlib", "newmod.pyi"]), ""),
            tf(path(&["ts", "stdlib", "bye.pyi"]), ""),
            tf(path(&["ts", "stdlib", "winonly.pyi"]), ""),
            tf(path(&["ts", "stdlib", "typing_extensions.pyi"]), ""),
            tf(
                path(&["ts", "stubs", "requests", "requests", "__init__.pyi"]),
                "",
            ),
            tf(path(&["src", "main.py"]), ""),
        ]
    }

    #[test]
    fn stdlib_typeshed_honors_versions() {
        let s = setup_import_resolver(&typeshed_files(), |c| {
            c.typeshed_path = Some(file("/ts"));
            c.default_python_version = Some(PYTHON_VERSION3_9);
            c.default_python_platform = Some("Linux".to_string());
        });

        let os = s.resolve(&["os"]);
        assert!(os.is_import_found);
        assert_eq!(os.import_type, ImportType::BuiltIn);
        assert_eq!(os.is_stdlib_typeshed_file, Some(true));

        assert!(!s.resolve(&["newmod"]).is_import_found);
        assert!(s.resolve(&["bye"]).is_import_found);
        assert!(!s.resolve(&["winonly"]).is_import_found);

        let typing_extensions = s.resolve(&["typing_extensions"]);
        assert_eq!(typing_extensions.import_type, ImportType::ThirdParty);

        let requests = s.resolve(&["requests"]);
        assert!(requests.is_import_found);
        assert_eq!(requests.import_type, ImportType::ThirdParty);
        assert_eq!(requests.is_third_party_typeshed_file, Some(true));
    }

    #[test]
    fn typeshed_stdlib_exclude_list() {
        let s = setup_import_resolver(&typeshed_files(), |_| {});

        let excludes = s.import_resolver.get_typeshed_stdlib_exclude_list(
            Some(file("/ts")),
            PYTHON_VERSION3_13,
            Some("Linux"),
        );
        let excludes: Vec<&str> = excludes.iter().map(|u| u.get_file_path()).collect();
        assert_eq!(
            excludes,
            [
                "/ts/stdlib/bye",
                "/ts/stdlib/bye.pyi",
                "/ts/stdlib/winonly",
                "/ts/stdlib/winonly.pyi"
            ]
        );
    }

    #[test]
    fn module_name_for_typeshed_and_invalid_names() {
        let mut files = typeshed_files();
        files.push(tf(path(&["src", "my-pkg", "mod.py"]), ""));
        let s = setup_import_resolver(&files, |c| c.typeshed_path = Some(file("/ts")));
        let exec_env = s.config_options.find_exec_environment(s.uri);

        let os = s.import_resolver.get_module_name_for_import(
            file("/ts/stdlib/os/__init__.pyi"),
            &exec_env,
            false,
            false,
        );
        assert_eq!(os.module_name, "os");
        assert!(os.is_typeshed_file);
        assert_eq!(os.import_type, ImportType::BuiltIn);

        let requests = s.import_resolver.get_module_name_for_import(
            file("/ts/stubs/requests/requests/__init__.pyi"),
            &exec_env,
            false,
            false,
        );
        assert_eq!(requests.module_name, "requests");
        assert_eq!(requests.import_type, ImportType::ThirdParty);

        let invalid = file("/src/my-pkg/mod.py");
        assert_eq!(
            s.import_resolver
                .get_module_name_for_import(invalid, &exec_env, false, false)
                .module_name,
            ""
        );
        assert_eq!(
            s.import_resolver
                .get_module_name_for_import(invalid, &exec_env, true, false)
                .module_name,
            "src.my-pkg.mod"
        );
    }

    #[test]
    fn import_roots_order() {
        let mut files = typeshed_files();
        files.push(tf(path(&["typings", "x.pyi"]), ""));
        files.push(tf(path(&["extra", "y.py"]), ""));
        files.push(tf(lib(&["z.py"]), ""));
        let s = setup_import_resolver(&files, |c| {
            c.typeshed_path = Some(file("/ts"));
            c.stub_path = Some(file("/typings"));
            c.default_extra_paths = Some(vec![file("/extra")]);
        });
        let exec_env = s.config_options.find_exec_environment(s.uri);

        let roots = |for_logging: bool| -> Vec<String> {
            let roots = s.import_resolver.get_import_roots(&exec_env, for_logging);
            roots
                .iter()
                .map(|u| u.get_file_path().to_string())
                .collect()
        };
        assert_eq!(
            roots(false),
            [
                "/ts/stdlib",
                "/",
                "/extra",
                "/typings",
                "/ts/stubs/requests",
                "/lib/site-packages"
            ]
        );
        assert_eq!(
            roots(true),
            [
                "/ts/stdlib",
                "/",
                "/extra",
                "/typings",
                "/ts/stubs/...",
                "/lib/site-packages"
            ]
        );
    }

    #[test]
    fn verbose_failure_info_holds_every_line_of_the_search() {
        let files = [
            tf(lib(&["other.py"]), ""),
            tf(path(&["src", "main.py"]), ""),
        ];
        let s = setup_import_resolver(&files, |c| c.verbose_output = Some(true));

        let result = s.resolve(&["other", "missing"]);
        assert!(!result.is_import_found);
        let info = result
            .import_failure_info
            .as_ref()
            .expect("importFailureInfo");
        // The site-packages result was created mid-search; it still carries the
        // lines logged after it, as pyright's shared array does.
        assert_eq!(
            info.last().map(String::as_str),
            Some("Typeshed path not found")
        );
        assert!(
            info.iter()
                .any(|l| l == "Looking for typeshed third-party path")
        );
    }

    #[test]
    fn real_file_system_resolution() {
        use crate::common::real_file_system::{RealTempFile, create_from_real_file_system};

        let root =
            std::env::temp_dir().join(format!("pyright-rs-import-resolver-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (rel, content) in [
            ("proj/pkg/__init__.py", ""),
            ("proj/pkg/sub.py", ""),
            ("proj/main.py", "import pkg.sub"),
            ("venv/lib/site-packages/thirdp/__init__.pyi", ""),
            ("venv/lib/site-packages/thirdp/__init__.py", ""),
        ] {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, content).unwrap();
        }
        let root_str = root.to_string_lossy().into_owned();

        let detector: &'static RealTempFile = Box::leak(Box::default());
        let console: &'static NullConsole = Box::leak(Box::default());
        let real_fs = Box::leak(Box::new(create_from_real_file_system(detector, console)));
        let fs: &'static PyrightFileSystem = Box::leak(Box::new(PyrightFileSystem::new(real_fs)));
        let sp = ServiceProvider::new(fs, console, detector);
        let uri_of = |rel: &str| Uri::file(&format!("{root_str}/{rel}"), detector, false);

        let config_options = P::new(ConfigOptions::new(uri_of("proj")));
        let host = TestAccessHost::leak(Uri::empty(), vec![uri_of("venv/lib/site-packages")], None);
        let import_resolver = ImportResolver::new(sp, config_options, host);
        let main = uri_of("proj/main.py");
        let exec_env = config_options.find_exec_environment(main);

        let result = import_resolver.resolve_import(main, &exec_env, &descriptor(&["pkg", "sub"]));
        assert!(result.is_import_found);
        assert_eq!(result.import_type, ImportType::Local);
        let resolved: Vec<&str> = result
            .resolved_uris
            .iter()
            .map(|u| u.get_file_path())
            .collect();
        assert_eq!(
            resolved,
            [
                format!("{root_str}/proj/pkg/__init__.py"),
                format!("{root_str}/proj/pkg/sub.py")
            ]
        );

        let result = import_resolver.resolve_import(main, &exec_env, &descriptor(&["thirdp"]));
        assert!(result.is_import_found);
        assert!(result.is_stub_file);
        assert_eq!(result.import_type, ImportType::ThirdParty);
        let non_stub = result.non_stub_import_result.expect("nonStubImportResult");
        assert_eq!(
            last_resolved(&non_stub),
            format!("{root_str}/venv/lib/site-packages/thirdp/__init__.py")
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
