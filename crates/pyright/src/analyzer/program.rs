//! Port of `packages/pyright-internal/src/analyzer/program.ts`.
//!
//! Ported for the CLI check: tracked files, the import graph, parse, bind,
//! check, import-cycle detection and diagnostics. Not ported: edit mode, open
//! files, notebook cell chains, shadow files, source mappers, cloning, the
//! realpath alias index (it only bridges invalidation between symlink twins
//! when files change, which never happens in a CLI run) and the cache manager
//! (`_handleMemoryHighUsage`; reference runs never empty the type cache).
//! The evaluator is created only where the constructor, `setConfigOptions` and
//! `setImportResolver` create it; the CLI never empties the cache.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use jsrt::{JsMap, JsSet, P};

use crate::common::timing::Duration;
use crate::common::timing::timing_stats;
use jsrt::number::number_to_string;
use crate::analyzer::analyzer_file_info::{ImportLookup, ImportLookupKey, ImportLookupResult, LookupImportOptions};
use crate::analyzer::analyzer_node_info::{
    AnalyzerNodeInfoContext, AnalyzerNodeInfoContextImpl, AnalyzerNodeInfoReader, get_dunder_all_info, get_file_info,
    get_scope,
};
use crate::analyzer::circular_dependency::CircularDependency;
use crate::analyzer::parse_tree_utils::get_doc_string;
use crate::analyzer::scope::Scope;
use crate::analyzer::symbol::SymbolTable;
use crate::analyzer::import_resolver::{ImportResolver, ImportedModuleDescriptor, ModuleImportInfo};
use crate::analyzer::import_result::{ImportResult, ImportType};
use crate::analyzer::source_file::{IPythonMode, SourceFile};
use crate::analyzer::source_file_info::{OptionalArguments, SourceFileInfo, is_user_code};
use crate::analyzer::type_evaluator::{EvaluatorOptions, TypeEvaluator, create_type_evaluator};
use crate::analyzer::type_printer::get_print_type_flags;
use crate::common::config_options::{ConfigOptions, DiagnosticLevel, match_file_specs};
use crate::common::console::ConsoleInterface;
use crate::common::debug::{assert, fail};
use crate::common::diagnostic::{Diagnostic, DiagnosticCategory};
use crate::common::diagnostic_sink::FileDiagnostics;
use crate::common::file_system::FileSystem;
use crate::common::service_provider::ServiceProvider;
use crate::common::uri::Uri;

// analyzer/program.ts:56
const MAX_IMPORT_DEPTH: i32 = 256;

// analyzer/program.ts:60
fn is_tagged_hint_diagnostic(diag: &Diagnostic) -> bool {
    matches!(
        diag.category,
        DiagnosticCategory::UnreachableCode | DiagnosticCategory::UnusedCode | DiagnosticCategory::Deprecated
    )
}

// analyzer/program.ts:67
#[derive(Clone, Copy, Debug)]
pub struct MaxAnalysisTime {
    pub open_files_time_in_ms: f64,
    pub no_open_files_time_in_ms: f64,
}

// analyzer/program.ts:80
struct UpdateImportInfo {
    path: Uri,
    is_typeshed_file: bool,
    is_third_party_import: bool,
    is_py_typed_present: bool,
}

/// `RequiringAnalysisCount` from `analysis.ts`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RequiringAnalysisCount {
    pub files: u32,
    pub cells: u32,
}

// analyzer/program.ts:146
/// Shared: `P<Program>`. The import lookup closure re-enters the program
/// while a file binds, so every method takes `&self` and no borrow is held
/// across a call.
pub struct Program {
    console: &'static dyn ConsoleInterface,
    source_file_list: RefCell<Vec<P<SourceFileInfo>>>,
    source_file_map: RefCell<JsMap<&'static str, P<SourceFileInfo>>>,
    allowed_third_party_imports: RefCell<Option<Vec<String>>>,
    config_options: Cell<P<ConfigOptions>>,
    import_resolver: Cell<P<ImportResolver>>,
    parsed_file_count: Cell<u32>,
    disable_checker: bool,
    look_up_import: OnceCell<ImportLookup>,
    self_handle: OnceCell<P<Program>>,
    analyzer_node_info_context: P<AnalyzerNodeInfoContextImpl>,
    evaluator: Cell<Option<P<TypeEvaluator>>>,
    pub service_provider: ServiceProvider,
}

impl Program {
    // analyzer/program.ts:182
    pub fn new(
        initial_import_resolver: P<ImportResolver>,
        initial_config_options: P<ConfigOptions>,
        service_provider: ServiceProvider,
        disable_checker: bool,
    ) -> P<Program> {
        let program = P::new(Program {
            console: service_provider.console,
            source_file_list: RefCell::new(Vec::new()),
            source_file_map: RefCell::new(JsMap::default()),
            allowed_third_party_imports: RefCell::new(None),
            config_options: Cell::new(initial_config_options),
            import_resolver: Cell::new(initial_import_resolver),
            parsed_file_count: Cell::new(0),
            disable_checker,
            look_up_import: OnceCell::new(),
            self_handle: OnceCell::new(),
            analyzer_node_info_context: AnalyzerNodeInfoContextImpl::new(),
            evaluator: Cell::new(None),
            service_provider,
        });

        let look_up_import: ImportLookup =
            Rc::new(move |file_uri_or_module, options| program.look_up_import_impl(file_uri_or_module, options));
        let _ = program.look_up_import.set(look_up_import);
        let _ = program.self_handle.set(program);
        program.create_new_evaluator();
        program
    }

    // analyzer/program.ts:209
    pub fn console(&self) -> &'static dyn ConsoleInterface {
        self.console
    }

    // analyzer/program.ts:213
    pub fn root_path(&self) -> Uri {
        self.config_options.get().project_root
    }

    // analyzer/program.ts:217
    pub fn evaluator(&self) -> Option<P<TypeEvaluator>> {
        self.evaluator.get()
    }

    // analyzer/program.ts:221
    pub fn config_options(&self) -> P<ConfigOptions> {
        self.config_options.get()
    }

    // analyzer/program.ts:225
    pub fn import_resolver(&self) -> P<ImportResolver> {
        self.import_resolver.get()
    }

    // analyzer/program.ts:229
    pub fn file_system(&self) -> &'static dyn FileSystem {
        self.import_resolver.get().file_system()
    }

    // analyzer/program.ts:236
    pub fn look_up_import(&self) -> ImportLookup {
        self.look_up_import.get().unwrap().clone()
    }

    // analyzer/program.ts:248
    pub fn analyzer_node_info_reader(&self) -> &'static dyn AnalyzerNodeInfoReader {
        self.analyzer_node_info_context.get()
    }

    // analyzer/program.ts:327
    pub fn set_config_options(&self, config_options: P<ConfigOptions>) {
        self.config_options.set(config_options);
        self.import_resolver.get().set_config_options(config_options);

        // Create a new evaluator with the updated config options.
        self.create_new_evaluator();
    }

    // analyzer/program.ts:335
    pub fn set_import_resolver(&self, import_resolver: P<ImportResolver>) {
        self.import_resolver.set(import_resolver);

        // Create a new evaluator with the updated import resolver.
        // Otherwise, lookup import passed to type evaluator might use
        // older import resolver when resolving imports after parsing.
        self.create_new_evaluator();
    }

    // analyzer/program.ts:345
    pub fn set_tracked_files(&self, file_uris: &[Uri]) -> Vec<FileDiagnostics> {
        if !self.source_file_list.borrow().is_empty() {
            // We need to determine which files to remove from the existing file list.
            let mut new_file_map: JsMap<&'static str, Uri> = JsMap::default();
            for path in file_uris {
                new_file_map.insert(path.key(), *path);
            }

            // Files that are not in the tracked file list are marked as no longer tracked,
            // but only for non-virtual files that participate in source enumeration.
            for old_file in self.source_file_list.borrow().iter() {
                if !new_file_map.contains_key(old_file.uri().key()) && !old_file.is_virtual() {
                    old_file.set_is_tracked(false);
                }
            }
        }

        // Add the new files. Only the new items will be added.
        self.add_tracked_files(file_uris, false, false);

        self.remove_unneeded_files()
    }

    // analyzer/program.ts:381
    pub fn set_allowed_third_party_imports(&self, import_names: Vec<String>) {
        *self.allowed_third_party_imports.borrow_mut() = Some(import_names);
    }

    // analyzer/program.ts:385
    pub fn add_tracked_files(&self, file_uris: &[Uri], is_third_party_import: bool, is_in_py_typed_package: bool) {
        for file_uri in file_uris {
            self.add_tracked_file(*file_uri, is_third_party_import, is_in_py_typed_package);
        }
    }

    // analyzer/program.ts:401
    pub fn add_tracked_file(&self, file_uri: Uri, is_third_party_import: bool, is_in_py_typed_package: bool) -> P<SourceFile> {
        if let Some(source_file_info) = self.get_source_file_info(file_uri) {
            // The module name may have changed based on updates to the
            // search paths. Clear any cached module name so it is recomputed.
            source_file_info.source_file.clear_cached_module_name();
            source_file_info.set_is_tracked(true);
            return source_file_info.source_file;
        }

        // Detect py.typed status if not explicitly provided.
        let mut effective_is_in_py_typed_package = is_in_py_typed_package;
        if !is_in_py_typed_package {
            let module_import_info = self.get_module_import_info_for_file(file_uri);
            effective_is_in_py_typed_package = module_import_info.is_third_party_py_typed_present;
        }

        let source_file = self.create_source_file(file_uri, is_third_party_import, effective_is_in_py_typed_package);

        // Set the initial diagnostic rule set from the execution environment
        // so the file has config-level overrides from the start.
        let exec_env = self.config_options.get().find_exec_environment(file_uri);
        source_file.set_initial_diagnostic_rule_set(&exec_env.diagnostic_rule_set);
        let source_file_info = SourceFileInfo::new(
            source_file,
            source_file.is_typing_stub_file() || source_file.is_typeshed_stub_file() || source_file.is_built_in_stub_file(),
            is_third_party_import,
            effective_is_in_py_typed_package,
            OptionalArguments { is_tracked: Some(true), ..Default::default() },
        );
        self.add_to_source_file_list_and_map(source_file_info);
        source_file
    }

    /// `this._sourceFileFactory.createSourceFile(...)` with the default factory.
    fn create_source_file(&self, file_uri: Uri, is_third_party_import: bool, is_third_party_py_typed_present: bool) -> P<SourceFile> {
        let program = self.self_handle();
        SourceFile::new(
            self.service_provider.fs,
            file_uri,
            Rc::new(move |uri| program.get_module_name(uri)),
            is_third_party_import,
            is_third_party_py_typed_present,
            self.console,
            None,
        )
    }

    /// The `P` that owns `self`, for closures that outlive the call.
    fn self_handle(&self) -> P<Program> {
        *self.self_handle.get().unwrap()
    }

    // analyzer/program.ts:572
    pub fn mark_all_files_dirty(&self, even_if_contents_are_same: bool) {
        let mut mark_dirty_set: JsSet<&'static str> = JsSet::default();

        for source_file_info in self.get_source_file_info_list() {
            if even_if_contents_are_same {
                source_file_info.source_file.mark_dirty();
            } else if source_file_info.source_file.did_contents_change_on_disk() {
                source_file_info.source_file.mark_dirty();

                // Mark any files that depend on this file as dirty
                // also. This will retrigger analysis of these other files.
                self.mark_file_dirty_recursive(source_file_info, &mut mark_dirty_set, false);
            }
        }
    }

    // analyzer/program.ts:632
    pub fn get_file_count(&self, user_file_only: bool) -> usize {
        let list = self.source_file_list.borrow();
        if user_file_only {
            return list.iter().filter(|f| is_user_code(Some(**f))).count();
        }

        list.len()
    }

    // analyzer/program.ts:642
    pub fn get_user_file_count(&self) -> usize {
        self.source_file_list.borrow().iter().filter(|s| is_user_code(Some(**s))).count()
    }

    // analyzer/program.ts:646
    pub fn get_user_files(&self) -> Vec<P<SourceFileInfo>> {
        self.source_file_list.borrow().iter().copied().filter(|s| is_user_code(Some(*s))).collect()
    }

    // analyzer/program.ts:664
    pub fn get_files_to_analyze_count(&self) -> RequiringAnalysisCount {
        let mut files_to_analyze_count = 0;
        let mut cells_to_analyze_count = 0;

        if self.disable_checker {
            return RequiringAnalysisCount { files: 0, cells: 0 };
        }

        for file_info in self.get_source_file_info_list() {
            let source_file = file_info.source_file;
            if source_file.is_checking_required() && self.should_check_file(file_info) {
                if source_file.get_ipython_mode() == IPythonMode::CellDocs {
                    cells_to_analyze_count += 1;
                } else {
                    files_to_analyze_count += 1;
                }
            }
        }

        RequiringAnalysisCount { files: files_to_analyze_count, cells: cells_to_analyze_count }
    }

    // analyzer/program.ts:689
    pub fn is_checking_only_open_files(&self) -> bool {
        self.config_options.get().check_only_open_files.unwrap_or(false)
    }

    // analyzer/program.ts:707
    pub fn owns(&self, uri: Uri) -> bool {
        if let Some(file_info) = self.get_source_file_info(uri) {
            // If we already determined whether the file is tracked or not, don't do it again.
            return file_info.is_tracked();
        }

        match_file_specs(&self.config_options.get(), uri, true)
    }

    // analyzer/program.ts:718
    pub fn get_source_file(&self, uri: Uri) -> Option<P<SourceFile>> {
        self.get_source_file_info(uri).map(|info| info.source_file)
    }

    // analyzer/program.ts:731
    /// A snapshot of the list.
    pub fn get_source_file_info_list(&self) -> Vec<P<SourceFileInfo>> {
        self.source_file_list.borrow().clone()
    }

    // analyzer/program.ts:735
    pub fn get_source_file_info(&self, uri: Uri) -> Option<P<SourceFileInfo>> {
        if !uri.is_empty() {
            return self.source_file_map.borrow().get(uri.key()).copied();
        }
        None
    }

    // analyzer/program.ts:750
    pub fn get_bound_source_file_info(&self, uri: Uri, content: Option<&str>, force: bool) -> Option<P<SourceFileInfo>> {
        let source_file_info = self.get_source_file_info(uri)?;
        self.bind_file(source_file_info, content, force, false);
        Some(source_file_info)
    }

    // analyzer/program.ts:766
    /// No cancellation token: the CLI never cancels.
    pub fn analyze(&self, max_time: Option<MaxAnalysisTime>) -> bool {
        let elapsed_time = Duration::new();

        let open_files: Vec<P<SourceFileInfo>> = self
            .get_source_file_info_list()
            .into_iter()
            .filter(|sf| sf.is_open_by_client() && sf.source_file.is_checking_required())
            .collect();

        if !open_files.is_empty() {
            let effective_max_time = max_time.map_or(f64::MAX, |t| t.open_files_time_in_ms);

            // Check the open files.
            for source_file_info in open_files {
                if self.check_types(source_file_info, false) && elapsed_time.get_duration_in_milliseconds() > effective_max_time {
                    return true;
                }
            }

            // If the caller specified a maxTime, return at this point
            // since we've finalized all open files.
            if max_time.is_some() {
                return true;
            }
        }

        if !self.config_options.get().check_only_open_files.unwrap_or(false) {
            let effective_max_time = max_time.map_or(f64::MAX, |t| t.no_open_files_time_in_ms);

            // Now do type parsing and analysis of the remaining. `for...of`
            // over an array sees elements appended during the loop.
            let mut index = 0;
            loop {
                let Some(source_file_info) = self.source_file_list.borrow().get(index).copied() else {
                    break;
                };
                index += 1;

                if !is_user_code(Some(source_file_info)) {
                    continue;
                }

                if self.check_types(source_file_info, false) && elapsed_time.get_duration_in_milliseconds() > effective_max_time {
                    return true;
                }
            }
        }

        false
    }

    // analyzer/program.ts:898
    /// Prints the files that have been checked and their check times,
    /// greatest first.
    pub fn print_detailed_analysis_times(&self) {
        let mut sorted_files: Vec<P<SourceFileInfo>> = self
            .get_source_file_info_list()
            .into_iter()
            .filter(|s| s.source_file.get_check_time().is_some())
            .collect();
        jsrt::sort::sort_by(&mut sorted_files, |a, b| {
            b.source_file.get_check_time().unwrap() - a.source_file.get_check_time().unwrap()
        });

        self.console.info("");
        self.console.info("Analysis time by file");

        for sf_info in sorted_files {
            let check_time_in_ms = sf_info.source_file.get_check_time().unwrap();
            self.console.info(&format!("{}ms: {}", number_to_string(check_time_in_ms), sf_info.uri().to_uri_string()));
        }
    }

    // analyzer/program.ts:1011
    pub fn get_diagnostics(&self, options: &ConfigOptions, report_deltas_only: bool) -> Vec<FileDiagnostics> {
        let mut file_diagnostics = self.remove_unneeded_files();

        for source_file_info in self.get_source_file_info_list() {
            if self.should_check_file(source_file_info) {
                let diagnostics = source_file_info.source_file.get_diagnostics(
                    options,
                    if report_deltas_only { source_file_info.diagnostics_version() } else { None },
                );

                if let Some(mut diagnostics) = diagnostics {
                    // Filter out all categories that are translated to tagged hints?
                    if options.disable_tagged_hints {
                        diagnostics.retain(|diag| !is_tagged_hint_diagnostic(diag));
                    }

                    file_diagnostics.push(FileDiagnostics {
                        file_uri: source_file_info.uri(),
                        version: source_file_info.source_file.get_client_version(),
                        diagnostics,
                    });

                    // Update the cached diagnosticsVersion so we can determine
                    // whether there are any updates next time we call getDiagnostics.
                    source_file_info.set_diagnostics_version(Some(source_file_info.source_file.get_diagnostic_version()));
                }
            } else if !source_file_info.is_open_by_client()
                && options.check_only_open_files.unwrap_or(false)
                && source_file_info.diagnostics_version().is_some()
            {
                // This condition occurs when the user switches from workspace to
                // "open files only" mode. Clear all diagnostics for this file.
                file_diagnostics.push(FileDiagnostics {
                    file_uri: source_file_info.uri(),
                    version: source_file_info.source_file.get_client_version(),
                    diagnostics: Vec::new(),
                });
                source_file_info.set_diagnostics_version(None);
            }
        }

        file_diagnostics
    }

    // analyzer/program.ts:1304
    fn remove_unneeded_files(&self) -> Vec<FileDiagnostics> {
        let mut file_diagnostics = Vec::new();

        // If a file is no longer tracked, opened or shadowed, it can
        // be removed from the program.
        let mut i = 0;
        loop {
            let Some(file_info) = self.source_file_list.borrow().get(i).copied() else {
                break;
            };

            if !self.is_file_needed(file_info, false) {
                // Clear only if there are any errors for this file.
                if file_info.diagnostics_version().is_some() {
                    file_diagnostics.push(FileDiagnostics {
                        file_uri: file_info.uri(),
                        version: file_info.source_file.get_client_version(),
                        diagnostics: Vec::new(),
                    });
                }

                file_info.source_file.prepare_for_close();
                self.remove_source_file_from_list_and_map(file_info.uri(), i);

                // Unlink any imports and remove them from the list if
                // they are no longer referenced.
                for imported_file in file_info.imports() {
                    let Some(index_to_remove) = imported_file.imported_by().iter().position(|fi| P::ptr_eq(*fi, file_info))
                    else {
                        continue;
                    };

                    imported_file.mutate().imported_by.remove(index_to_remove);

                    // See if we need to remove the imported file because it
                    // is no longer needed. If its index is >= i, it will be
                    // removed when we get to it.
                    if !self.is_file_needed(imported_file, false) {
                        let index_to_remove =
                            self.source_file_list.borrow().iter().position(|fi| P::ptr_eq(*fi, imported_file));
                        if let Some(index_to_remove) = index_to_remove.filter(|&index| index < i) {
                            // Clear if there are any errors for this import.
                            if imported_file.diagnostics_version().is_some() {
                                file_diagnostics.push(FileDiagnostics {
                                    file_uri: imported_file.uri(),
                                    version: imported_file.source_file.get_client_version(),
                                    diagnostics: Vec::new(),
                                });
                            }

                            imported_file.source_file.prepare_for_close();
                            self.remove_source_file_from_list_and_map(imported_file.uri(), index_to_remove);
                            i -= 1;
                        }
                    }
                }

                // Remove any shadowed files corresponding to this file.
                for shadowed_file in file_info.shadowed_by() {
                    shadowed_file.mutate().shadows.retain(|f| !P::ptr_eq(*f, file_info));
                }
                file_info.mutate().shadowed_by = Vec::new();
            } else {
                // If we're showing the user errors only for open files, clear
                // out the errors for the now-closed file.
                if !self.should_check_file(file_info) && file_info.diagnostics_version().is_some() {
                    file_diagnostics.push(FileDiagnostics {
                        file_uri: file_info.uri(),
                        version: file_info.source_file.get_client_version(),
                        diagnostics: Vec::new(),
                    });
                    file_info.set_diagnostics_version(None);
                }

                i += 1;
            }
        }

        file_diagnostics
    }

    // analyzer/program.ts:1380
    fn is_file_needed(&self, file_info: P<SourceFileInfo>, skip_file_needed_check: bool) -> bool {
        if file_info.source_file.is_file_deleted() {
            return false;
        }

        if skip_file_needed_check || file_info.is_tracked() || file_info.is_open_by_client() {
            return true;
        }

        if !file_info.shadows().is_empty() {
            return true;
        }

        if file_info.imported_by().is_empty() {
            return false;
        }

        // It's possible for a cycle of files to be imported
        // by a tracked file but then abandoned.
        self.is_import_needed_recursive(file_info, &mut JsSet::default())
    }

    // analyzer/program.ts:1404
    fn is_import_needed_recursive(&self, file_info: P<SourceFileInfo>, recursion_set: &mut JsSet<&'static str>) -> bool {
        if file_info.is_tracked() || file_info.is_open_by_client() || !file_info.shadows().is_empty() {
            return true;
        }

        let file_uri = file_info.uri();

        // Avoid infinite recursion.
        if recursion_set.contains(file_uri.key()) {
            return false;
        }

        recursion_set.insert(file_uri.key());

        for importer_info in file_info.imported_by() {
            if self.is_import_needed_recursive(importer_info, recursion_set) {
                return true;
            }
        }

        false
    }

    // analyzer/program.ts:1467
    fn is_import_allowed(&self, importer: P<SourceFileInfo>, import_result: &ImportResult, is_import_stub_file: bool) -> bool {
        // Don't import native libs. We don't want to track these files,
        // and we definitely don't want to attempt to parse them.
        if import_result.is_native_lib {
            return false;
        }

        let config_options = self.config_options.get();
        let use_library_code_for_types = config_options.use_library_code_for_types.unwrap_or(false);
        let mut third_party_import_allowed = use_library_code_for_types
            || (import_result.import_type == ImportType::ThirdParty && import_result.py_typed_info.is_some())
            || (import_result.import_type == ImportType::Local && importer.is_third_party_py_typed_present);

        if import_result.import_type == ImportType::ThirdParty
            || (importer.is_third_party_import && import_result.import_type == ImportType::Local)
        {
            if let Some(allowed_third_party_imports) = self.allowed_third_party_imports.borrow().as_ref() {
                if import_result.is_relative {
                    // If it's a relative import, we'll allow it because the
                    // importer was already deemed to be allowed.
                    third_party_import_allowed = true;
                } else if allowed_third_party_imports.iter().any(|import_name| {
                    // If this import name is the one that was explicitly
                    // allowed or is a child of that import name,
                    // it's considered allowed.
                    import_result.import_name == *import_name
                        || import_result.import_name.starts_with(&format!("{import_name}."))
                }) {
                    third_party_import_allowed = true;
                }
            } else if importer.is_third_party_import && use_library_code_for_types {
                // If the importing file is a third-party import, allow importing of
                // additional third-party imports.
                third_party_import_allowed = true;
            } else if import_result.is_namespace_package
                && import_result
                    .filtered_implicit_imports
                    .as_ref()
                    .is_some_and(|imports| imports.values().any(|implicit_import| implicit_import.py_typed_info.is_some()))
            {
                // Handle the case where the import targets a namespace package, and a
                // submodule contained within it has a py.typed marker.
                third_party_import_allowed = true;
            }

            // Don't explore any third-party files unless they're type stub files
            // or we've been told explicitly that third-party imports are OK.
            if !is_import_stub_file {
                return third_party_import_allowed;
            }
        }

        true
    }

    // analyzer/program.ts:1537
    fn get_source_file_info_from_key(&self, key: &str) -> Option<P<SourceFileInfo>> {
        self.source_file_map.borrow().get(key).copied()
    }

    // analyzer/program.ts:1541
    fn update_source_file_imports(&self, source_file_info: P<SourceFileInfo>, options: &ConfigOptions) -> Vec<P<SourceFileInfo>> {
        let mut files_added: Vec<P<SourceFileInfo>> = Vec::new();

        // Get the new list of imports and see if it changed from the last
        // list of imports for this file.
        let imports = source_file_info.source_file.get_imports();

        // An import is considered third-party if it is external to the importer
        // or is internal but the importer is itself a third-party package.
        let get_third_party_import_info = |import_result: &ImportResult| {
            let mut is_third_party_import = false;
            let mut is_py_typed_present = false;

            if import_result.import_type == ImportType::ThirdParty {
                is_third_party_import = true;
                if import_result.py_typed_info.is_some() {
                    is_py_typed_present = true;
                }
            } else if source_file_info.is_third_party_import && import_result.import_type == ImportType::Local {
                is_third_party_import = true;
                if source_file_info.is_third_party_py_typed_present {
                    is_py_typed_present = true;
                }
            }

            (is_third_party_import, is_py_typed_present)
        };

        // Create a map of unique imports, since imports can appear more than once.
        let mut new_import_path_map: JsMap<&'static str, UpdateImportInfo> = JsMap::default();

        // Add chained source file as import if it exists.
        if let Some(chained_source_file) = source_file_info.chained_source_file() {
            if chained_source_file.source_file.is_file_deleted() {
                source_file_info.set_chained_source_file(None);
            } else {
                let file_uri = chained_source_file.uri();
                new_import_path_map.insert(
                    file_uri.key(),
                    UpdateImportInfo { path: file_uri, is_typeshed_file: false, is_third_party_import: false, is_py_typed_present: false },
                );
            }
        }

        for import_result in &imports {
            if import_result.is_import_found {
                if self.is_import_allowed(source_file_info, import_result, import_result.is_stub_file) {
                    if let Some(&file_uri) = import_result.resolved_uris.last() {
                        if !file_uri.is_empty() {
                            let (is_third_party_import, is_py_typed_present) = get_third_party_import_info(import_result);
                            new_import_path_map.insert(
                                file_uri.key(),
                                UpdateImportInfo {
                                    path: file_uri,
                                    is_typeshed_file: import_result.is_stdlib_typeshed_file.unwrap_or(false)
                                        || import_result.is_third_party_typeshed_file.unwrap_or(false),
                                    is_third_party_import,
                                    is_py_typed_present,
                                },
                            );
                        }
                    }
                }

                if let Some(filtered_implicit_imports) = &import_result.filtered_implicit_imports {
                    for implicit_import in filtered_implicit_imports.values() {
                        if self.is_import_allowed(source_file_info, import_result, implicit_import.is_stub_file)
                            && !implicit_import.is_native_lib
                        {
                            let (is_third_party_import, is_py_typed_present) = get_third_party_import_info(import_result);
                            new_import_path_map.insert(
                                implicit_import.uri.key(),
                                UpdateImportInfo {
                                    path: implicit_import.uri,
                                    is_typeshed_file: import_result.is_stdlib_typeshed_file.unwrap_or(false)
                                        || import_result.is_third_party_typeshed_file.unwrap_or(false),
                                    is_third_party_import,
                                    is_py_typed_present,
                                },
                            );
                        }
                    }
                }

                // If the stub was found but the non-stub (source) file was not, dump
                // the failure to the log for diagnostic purposes.
                if let Some(non_stub_import_result) = import_result.non_stub_import_result.filter(|r| !r.is_import_found) {
                    if !source_file_info.source_file.is_stub_file()
                        && !import_result.is_stdlib_typeshed_file.unwrap_or(false)
                        && import_result.import_name != "__builtins__"
                        && options.verbose_output.unwrap_or(false)
                    {
                        self.console.info(&format!(
                            "Could not resolve source for '{}' in file '{}'",
                            import_result.import_name,
                            source_file_info.uri().to_user_visible_string()
                        ));

                        if let Some(import_failure_info) = &non_stub_import_result.import_failure_info {
                            for diag in import_failure_info {
                                self.console.info(&format!("  {diag}"));
                            }
                        }
                    }
                }
            } else if options.verbose_output.unwrap_or(false) {
                self.console.info(&format!(
                    "Could not import '{}' in file '{}'",
                    import_result.import_name,
                    source_file_info.uri().to_user_visible_string()
                ));
                if let Some(import_failure_info) = &import_result.import_failure_info {
                    for diag in import_failure_info {
                        self.console.info(&format!("  {diag}"));
                    }
                }
            }
        }

        let mut updated_import_map: JsMap<&'static str, P<SourceFileInfo>> = JsMap::default();
        for import_info in source_file_info.imports() {
            let old_file_path = import_info.uri();

            // A previous import was removed.
            if !new_import_path_map.contains_key(old_file_path.key()) {
                let importer_uri = source_file_info.uri();
                import_info.mutate().imported_by.retain(|fi| !fi.uri().equals(Some(importer_uri)));
            } else {
                updated_import_map.insert(old_file_path.key(), import_info);
            }
        }

        // See if there are any new imports to be added.
        for (normalized_import_path, import_info) in &new_import_path_map {
            if !updated_import_map.contains_key(normalized_import_path) {
                // We found a new import to add. See if it's already part
                // of the program.
                let imported_file_info = match self.get_source_file_info(import_info.path) {
                    Some(info) => info,
                    None => {
                        let source_file = self.create_source_file(
                            import_info.path,
                            import_info.is_third_party_import,
                            import_info.is_py_typed_present,
                        );
                        let imported_file_info = SourceFileInfo::new(
                            source_file,
                            import_info.is_typeshed_file,
                            import_info.is_third_party_import,
                            import_info.is_py_typed_present,
                            OptionalArguments::default(),
                        );

                        self.add_to_source_file_list_and_map(imported_file_info);
                        files_added.push(imported_file_info);
                        imported_file_info
                    }
                };

                imported_file_info.mutate().imported_by.push(source_file_info);
                updated_import_map.insert(normalized_import_path, imported_file_info);
            }
        }

        // Update the imports list. It should now map the set of imports
        // specified by the source file.
        let new_imports: Vec<P<SourceFileInfo>> =
            new_import_path_map.keys().filter_map(|key| self.get_source_file_info_from_key(key)).collect();

        // Mutate only when necessary to avoid extra binding operations.
        let old_imports = source_file_info.imports();
        if new_imports.len() != old_imports.len()
            || !new_imports.iter().all(|i| old_imports.iter().any(|old| P::ptr_eq(*old, *i)))
        {
            source_file_info.mutate().imports = new_imports;
        }

        // Resolve the builtins import for the file. This needs to be
        // analyzed before the file can be analyzed.
        source_file_info.set_builtins_import(None);
        if let Some(builtins_import) = source_file_info.source_file.get_builtins_import().filter(|b| b.is_import_found) {
            let resolved_builtins_path = *builtins_import.resolved_uris.last().unwrap();
            source_file_info.set_builtins_import(self.get_source_file_info(resolved_builtins_path));
        }

        files_added
    }

    // analyzer/program.ts:1745
    fn remove_source_file_from_list_and_map(&self, file_uri: Uri, index_to_remove: usize) {
        let source_file_info = self.source_file_map.borrow().get(file_uri.key()).copied();
        if let Some(source_file_info) = source_file_info {
            self.drop_parse_and_bind_info(source_file_info.source_file);
        }

        self.source_file_map.borrow_mut().shift_remove(file_uri.key());
        self.source_file_list.borrow_mut().remove(index_to_remove);
    }

    // analyzer/program.ts:1756
    fn drop_parse_and_bind_info(&self, source_file: P<SourceFile>) {
        if let Some(parse_tree) = source_file.drop_parse_and_bind_info() {
            self.analyzer_node_info_context.remove(parse_tree);
        }
    }

    // analyzer/program.ts:1763
    fn add_to_source_file_list_and_map(&self, file_info: P<SourceFileInfo>) {
        let file_uri = file_info.uri();

        // We should never add a file with the same path twice.
        assert(!self.source_file_map.borrow().contains_key(file_uri.key()), None);

        // We should never have an empty URI for a source file.
        assert(!file_uri.is_empty(), None);

        self.source_file_list.borrow_mut().push(file_info);
        self.source_file_map.borrow_mut().insert(file_uri.key(), file_info);
    }

    // analyzer/program.ts:1907
    fn get_module_name(&self, file_uri: Uri) -> String {
        self.get_module_import_info_for_file(file_uri).module_name
    }

    // analyzer/program.ts:1912
    fn get_module_import_info_for_file(&self, file_uri: Uri) -> ModuleImportInfo {
        // We allow illegal module names (e.g. names that include "-" in them)
        // because we want a unique name for each module even if it cannot be
        // imported through an "import" statement.
        let config_options = self.config_options.get();
        self.import_resolver.get().get_module_name_for_import(
            file_uri,
            &config_options.get_default_exec_environment(),
            true,
            true,
        )
    }

    // analyzer/program.ts:2008
    fn parse_file(&self, file_to_parse: P<SourceFileInfo>, content: Option<&str>, skip_file_needed_check: bool) {
        if !self.is_file_needed(file_to_parse, skip_file_needed_check) || !file_to_parse.source_file.is_parse_required() {
            return;
        }

        // SourceFile.parse should only be called here in the program.
        let config_options = self.config_options.get();
        if file_to_parse.source_file.parse(&config_options, &self.import_resolver.get(), content) {
            self.parsed_file_count.set(self.parsed_file_count.get() + 1);
            self.update_source_file_imports(file_to_parse, &config_options);
        }

        if file_to_parse.source_file.is_file_deleted() {
            file_to_parse.set_is_tracked(false);

            // Mark any files that depend on this file as dirty
            // also. This will retrigger analysis of these other files.
            let mut mark_dirty_set: JsSet<&'static str> = JsSet::default();
            self.mark_file_dirty_recursive(file_to_parse, &mut mark_dirty_set, false);

            // Invalidate the import resolver's cache as well.
            self.import_resolver.get().invalidate_cache();
        }
    }

    // analyzer/program.ts:2035
    fn get_implicit_imports(&self, file: P<SourceFileInfo>) -> Option<P<SourceFileInfo>> {
        // If file is builtins.pyi, then chainedSourceFile might not exist or be incorrect.
        if file.builtins_import().is_some_and(|b| P::ptr_eq(b, file)) {
            return None;
        }

        if let Some(chained) = file.chained_source_file().filter(|c| !c.source_file.is_file_deleted()) {
            return Some(chained);
        }

        file.builtins_import()
    }

    // analyzer/program.ts:2048
    fn bind_implicit_imports(&self, file_to_analyze: P<SourceFileInfo>, skip_file_needed_check: bool) {
        // Get all of the potential imports for this file.
        let mut implicit_imports: Vec<P<SourceFileInfo>> = Vec::new();
        let mut implicit_set: JsSet<&'static str> = JsSet::default();

        let mut next_implicit_import = self.get_implicit_imports(file_to_analyze);
        while let Some(next) = next_implicit_import {
            let implicit_path = next.uri();

            if implicit_set.contains(implicit_path.key()) {
                // We've found a cycle. Break out of the loop.
                fail(Some("Found a cycle in implicit imports files"));
            }

            implicit_set.insert(implicit_path.key());
            implicit_imports.push(next);

            self.parse_file(next, None, skip_file_needed_check);
            next_implicit_import = self.get_implicit_imports(next);
        }

        if implicit_imports.is_empty() {
            return;
        }

        // Reverse order, so top of chain is first.
        while let Some(implicit_import) = implicit_imports.pop() {
            // Bind this file but don't recurse into its imports.
            self.bind_file(implicit_import, None, skip_file_needed_check, true);
        }
    }

    // analyzer/program.ts:2088
    /// Binds the specified file. Returns true if the file was bound or it
    /// didn't need to be bound.
    fn bind_file(&self, file_to_bind: P<SourceFileInfo>, content: Option<&str>, skip_file_needed_check: bool, is_implicit_import: bool) -> bool {
        if !self.is_file_needed(file_to_bind, skip_file_needed_check) || !file_to_bind.source_file.is_binding_required() {
            return !file_to_bind.source_file.is_binding_required();
        }

        self.parse_file(file_to_bind, content, skip_file_needed_check);

        // Create a function to get the scope info.
        let get_scope_if_available = |file_info: Option<P<SourceFileInfo>>| -> Option<P<Scope>> {
            let file_info = file_info.filter(|f| !P::ptr_eq(*f, file_to_bind))?;

            // If the file was deleted, there's no scope to return.
            if file_info.source_file.is_file_deleted() {
                return None;
            }

            let parse_results = file_info.source_file.get_parser_output()?;

            // File should already be bound because of the chained file binding above.
            get_scope(parse_results.parse_tree, self.analyzer_node_info_context.get())
        };

        let mut builtins_scope: Option<P<Scope>> = None;
        if file_to_bind.builtins_import().is_some_and(|b| !P::ptr_eq(b, file_to_bind)) {
            // Bind all of the implicit imports first. So we don't recurse into them.
            if !is_implicit_import {
                self.bind_implicit_imports(file_to_bind, false);

                // Binding the implicit imports may indirectly cause the current file to be bound.
                // If so, return now to avoid "Bind called unnecessarily" assert in sourceFile.bind().
                if !file_to_bind.source_file.is_binding_required() {
                    return true;
                }
            }

            // If it is not builtin module itself, we need to parse and bind
            // the builtin module.
            builtins_scope = get_scope_if_available(file_to_bind.chained_source_file())
                .or_else(|| get_scope_if_available(file_to_bind.builtins_import()));
        }

        if file_to_bind.source_file.is_parse_required() {
            // Ensure the file is parsed before binding.
            self.parse_file(file_to_bind, content, skip_file_needed_check);
        }

        let mut future_imports = file_to_bind.source_file.get_parser_output().unwrap().future_imports.clone();
        if let Some(chained_source_file) = file_to_bind.chained_source_file() {
            future_imports = self.get_effective_future_imports(&future_imports, chained_source_file);
        }
        file_to_bind
            .set_effective_future_imports(if future_imports.is_empty() { None } else { Some(future_imports.clone()) });

        file_to_bind.source_file.bind(
            &self.config_options.get(),
            self.look_up_import(),
            builtins_scope,
            future_imports,
            self.analyzer_node_info_context,
        )
    }

    // analyzer/program.ts:2161
    fn get_effective_future_imports(&self, future_imports: &JsSet<String>, chained_source_file: P<SourceFileInfo>) -> JsSet<String> {
        let mut effective_future_imports = future_imports.clone();

        if let Some(chained) = chained_source_file.effective_future_imports() {
            for value in chained {
                effective_future_imports.insert(value);
            }
        }

        effective_future_imports
    }

    // analyzer/program.ts:2171
    fn look_up_import_impl(&self, file_uri_or_module: ImportLookupKey, options: Option<LookupImportOptions>) -> Option<ImportLookupResult> {
        let mut source_file_info: Option<P<SourceFileInfo>> = None;

        match file_uri_or_module {
            ImportLookupKey::Uri(uri) => {
                source_file_info = self.get_source_file_info(uri);
            }
            ImportLookupKey::Module(module) => {
                // Resolve the import.
                let config_options = self.config_options.get();
                let import_result = self.import_resolver.get().resolve_import(
                    module.importing_file_uri,
                    &config_options.find_exec_environment(module.importing_file_uri),
                    &ImportedModuleDescriptor {
                        leading_dots: 0,
                        name_parts: module.name_parts.clone(),
                        has_trailing_dot: None,
                        imported_symbols: None,
                    },
                );

                if import_result.is_import_found && !import_result.is_native_lib {
                    if let Some(&resolved_path) = import_result.resolved_uris.last() {
                        if !resolved_path.is_empty() {
                            // See if the source file already exists in the program.
                            source_file_info = self.get_source_file_info(resolved_path);

                            if source_file_info.is_none() {
                                // Start tracking the source file.
                                self.add_tracked_file(resolved_path, false, false);
                                source_file_info = self.get_source_file_info(resolved_path);
                            }
                        }
                    }
                }
            }
        }

        let source_file_info = source_file_info?;

        if options.and_then(|o| o.skip_parsing).unwrap_or(false) {
            // Return dummy information if the caller has indicated that parsing is
            // unnecessary.
            return Some(ImportLookupResult {
                symbol_table: SymbolTable::new(),
                dunder_all_names: None,
                uses_unsupported_dunder_all_form: false,
                doc_string: Rc::new(|| None),
                is_in_py_typed_package: false,
            });
        }

        if source_file_info.source_file.is_binding_required() {
            // Bind the file if it's not already bound. Don't count this time
            // against the type checker.
            timing_stats().type_checker_time.subtract_from_time(|| {
                self.bind_file(source_file_info, None, options.is_some_and(|o| o.skip_file_needed_check), false);
            });
        }

        let symbol_table = source_file_info.source_file.get_module_symbol_table()?;

        let parse_results = source_file_info.source_file.get_parser_output().unwrap();
        let module_node = parse_results.parse_tree;
        let reader = self.analyzer_node_info_context.get();
        let file_info = get_file_info(module_node, reader);

        let dunder_all_info = get_dunder_all_info(module_node, reader);

        Some(ImportLookupResult {
            symbol_table,
            dunder_all_names: dunder_all_info.map(|info| info.names.clone()),
            uses_unsupported_dunder_all_form: dunder_all_info.is_some_and(|info| info.uses_unsupported_dunder_all_form),
            // A getter in TypeScript: computed when read.
            doc_string: Rc::new(move || {
                get_doc_string(&module_node.d().statements.borrow()).map(|s| s.to_string_lossy())
            }),
            is_in_py_typed_package: file_info.is_in_py_typed_package,
        })
    }

    // analyzer/program.ts:1974
    /// `createNewEvaluatorInternal` is empty, and `createTypeEvaluatorWithTracker` without
    /// `logTypeEvaluationTime` is `createTypeEvaluator`.
    fn create_new_evaluator(&self) -> P<TypeEvaluator> {
        if let Some(evaluator) = self.evaluator.get() {
            // We shouldn't need to call this, but there appears to be a bug
            // in the v8 garbage collector where it's unable to resolve orphaned
            // objects without us giving it some assistance.
            evaluator.dispose_evaluator();
        }

        let config_options = self.config_options.get();
        let evaluator = create_type_evaluator(
            self.look_up_import(),
            EvaluatorOptions {
                print_type_flags: get_print_type_flags(config_options),
                log_calls: config_options.log_type_evaluation_time,
                minimum_logging_threshold: config_options.type_evaluation_time_threshold,
                evaluate_unknown_imports_as_any: config_options.evaluate_unknown_imports_as_any.unwrap_or(false),
                verify_type_cache_evaluator_flags: config_options.internal_test_mode.unwrap_or(false),
                node_info_reader: self.analyzer_node_info_context.to_dyn::<dyn AnalyzerNodeInfoReader>(|c| c),
            },
        );
        self.evaluator.set(Some(evaluator));
        evaluator
    }

    // analyzer/program.ts:2258
    fn should_check_file(&self, file_info: P<SourceFileInfo>) -> bool {
        // Always do a full checking for a file that's open in the editor.
        if file_info.is_open_by_client() {
            return true;
        }

        // If the file isn't currently open, only perform full checking for
        // files that are tracked, and only if the checkOnlyOpenFiles is disabled.
        !self.config_options.get().check_only_open_files.unwrap_or(false) && file_info.is_tracked()
    }

    // analyzer/program.ts:2273
    fn check_types(&self, file_to_check: P<SourceFileInfo>, skip_file_needed_check: bool) -> bool {
        // If the file isn't needed because it was eliminated from the
        // transitive closure or deleted, skip the file rather than wasting
        // time on it.
        if !self.is_file_needed(file_to_check, false) {
            return false;
        }

        if !file_to_check.source_file.is_checking_required() {
            return false;
        }

        if !skip_file_needed_check && !self.should_check_file(file_to_check) {
            return false;
        }

        // Bind the file if necessary even if we're not going to run the checker.
        let bound_file = self.bind_file(file_to_check, None, file_to_check.source_file.is_binding_required(), false);

        let config_options = self.config_options.get();
        // `_checkDependentFiles` returns undefined for a file that is not a notebook cell.
        if !self.disable_checker && bound_file {
            file_to_check.source_file.check(
                &config_options,
                self.look_up_import(),
                self.import_resolver.get(),
                self.evaluator.get().unwrap(),
                None,
                self.analyzer_node_info_context.to_dyn::<dyn AnalyzerNodeInfoReader>(|c| c),
            );
        }

        // Detect import cycles that involve the file.
        if config_options.diagnostic_rule_set.report_import_cycles != DiagnosticLevel::None {
            // Don't detect import cycles when doing type stub generation, or
            // for notebook cells.
            if self.allowed_third_party_imports.borrow().is_none() && file_to_check.ipython_mode() != IPythonMode::CellDocs {
                // We need to force all of the files to be parsed and build
                // a closure map for the files.
                let mut closure_map: JsMap<&'static str, P<SourceFileInfo>> = JsMap::default();
                self.get_imports_recursive(file_to_check, &mut closure_map, 0);

                for file in closure_map.values() {
                    timing_stats().cycle_detection_time.time_operation(|| {
                        let mut files_visited_map: JsMap<&'static str, P<SourceFileInfo>> = JsMap::default();

                        if !self.detect_and_report_import_cycles(
                            *file,
                            &mut files_visited_map,
                            &mut Vec::new(),
                            &mut JsMap::default(),
                        ) {
                            // If no cycles were found in any of the files we visited,
                            // set a flag to indicates that we don't need to visit them again
                            // on subsequent cycle checks.
                            for source_file_info in files_visited_map.values() {
                                source_file_info.source_file.set_no_circular_dependency_confirmed();
                            }
                        }
                    });
                }
            }
        }

        true
    }

    // analyzer/program.ts:2431
    fn get_imports_recursive(
        &self,
        file: P<SourceFileInfo>,
        closure_map: &mut JsMap<&'static str, P<SourceFileInfo>>,
        recursion_count: i32,
    ) {
        // If the file is already in the closure map, we found a cyclical
        // dependency. Don't recur further.
        let file_uri = file.uri();
        if closure_map.contains_key(file_uri.key()) {
            return;
        }

        // If the import chain is too long, emit an error. Otherwise we
        // risk blowing the stack.
        if recursion_count > MAX_IMPORT_DEPTH {
            file.source_file.set_hit_max_import_depth(MAX_IMPORT_DEPTH);
            return;
        }

        // Add the file to the closure map.
        closure_map.insert(file_uri.key(), file);

        // If this file hasn't already been parsed, parse it now. Skip this if
        // the file is part of a library.
        if is_user_code(Some(file)) {
            self.parse_file(file, None, false);
        }

        // Recursively add the file's imports.
        for imported_file_info in file.imports() {
            self.get_imports_recursive(imported_file_info, closure_map, recursion_count + 1);
        }
    }

    // analyzer/program.ts:2467
    fn detect_and_report_import_cycles(
        &self,
        source_file_info: P<SourceFileInfo>,
        files_visited: &mut JsMap<&'static str, P<SourceFileInfo>>,
        dependency_chain: &mut Vec<P<SourceFileInfo>>,
        dependency_map: &mut JsMap<&'static str, bool>,
    ) -> bool {
        // Don't bother checking for typestub files or third-party files.
        if source_file_info.source_file.is_stub_file() || source_file_info.is_third_party_import {
            return false;
        }

        // If we've already confirmed that this source file isn't part of a
        // cycle, we can skip it entirely.
        if source_file_info.source_file.is_no_circular_dependency_confirmed() {
            return false;
        }

        let file_uri = source_file_info.uri();

        files_visited.insert(file_uri.key(), source_file_info);

        let mut detected_cycle = false;

        if dependency_map.contains_key(file_uri.key()) {
            // We detect a cycle (partial or full). Report only full cycles,
            // rooted in the file at the start of the dependency chain.
            detected_cycle = true;

            // Look for chains at least two in length.
            if dependency_chain.len() > 1 && P::ptr_eq(source_file_info, dependency_chain[0]) {
                self.log_import_cycle(dependency_chain);
            }
        } else {
            // We use both a map (for fast lookups) and a list
            // (for ordering information).
            dependency_map.insert(file_uri.key(), true);
            dependency_chain.push(source_file_info);

            for imp in source_file_info.imports() {
                if self.detect_and_report_import_cycles(imp, files_visited, dependency_chain, dependency_map) {
                    detected_cycle = true;
                }
            }

            // Set the dependencyMap entry to false to indicate that we have
            // already explored this file and don't need to explore it again.
            dependency_map.insert(file_uri.key(), false);
            dependency_chain.pop();
        }

        detected_cycle
    }

    // analyzer/program.ts:2533
    fn log_import_cycle(&self, dependency_chain: &[P<SourceFileInfo>]) {
        let mut circ_dep = CircularDependency::default();
        for source_file_info in dependency_chain {
            circ_dep.append_path(source_file_info.uri());
        }

        circ_dep.normalize_order();
        let first_file_path = circ_dep.get_paths()[0];
        let first_source_file = self.get_source_file_info(first_file_path);
        assert(first_source_file.is_some(), None);
        first_source_file.unwrap().source_file.add_circular_dependency(&self.config_options.get(), circ_dep);
    }

    // analyzer/program.ts:2546
    fn mark_file_dirty_recursive(&self, source_file_info: P<SourceFileInfo>, mark_set: &mut JsSet<&'static str>, force_rebinding: bool) {
        let file_uri = source_file_info.uri();

        // Don't mark it again if it's already been visited.
        if mark_set.contains(file_uri.key()) {
            return;
        }

        source_file_info.source_file.mark_reanalysis_required(force_rebinding, self.analyzer_node_info_context.get());
        mark_set.insert(file_uri.key());

        for dep in source_file_info.imported_by() {
            // Changes on chained source file can change symbols in the symbol table and
            // dependencies on the dependent file. Force rebinding.
            let force_rebinding = dep.chained_source_file().is_some_and(|c| P::ptr_eq(c, source_file_info));
            self.mark_file_dirty_recursive(dep, mark_set, force_rebinding);
        }

        // Change in the current file could impact chained notebook cells.
        let mut chained_source_file = source_file_info.chained_source_file();
        while let Some(chained) = chained_source_file {
            if chained.source_file.is_checking_required() {
                // If the file is marked for checking, its chained one should be marked
                // as well. Stop here.
                break;
            }

            chained
                .source_file
                .mark_reanalysis_required(
                    source_file_info.ipython_mode() == IPythonMode::CellDocs,
                    self.analyzer_node_info_context.get(),
                );
            chained_source_file = chained.chained_source_file();
        }
    }
}
