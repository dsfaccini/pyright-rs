//! Port of `packages/pyright-internal/src/analyzer/service.ts`.
//!
//! Ported for the CLI check: config discovery and parsing, command-line
//! overrides, default options, source enumeration and the analysis run. Not
//! ported: file watchers (the CLI without `--watch` never creates them), open
//! files, edit mode, cloning and the library reanalysis timer.
//!
//! Node runs the analysis from a `setTimeout` that `scheduleReanalysis` arms.
//! Here `scheduleReanalysis` sets `analysis_timer`, and `run_timers` fires it
//! once the caller's synchronous work is done, as Node's event loop would.

use std::cell::{Cell, RefCell};

use jsrt::{JsSet, P};
use regex::Regex;
use serde_json::Value;

use crate::analyzer::analysis::{AnalysisCompleteCallback, AnalysisReason, AnalysisResults};
use crate::analyzer::background_analysis_program::BackgroundAnalysisProgram;
use crate::analyzer::import_logger::ImportLogger;
use crate::analyzer::import_resolver::ImportResolver;
use crate::analyzer::program::{MaxAnalysisTime, Program, RequiringAnalysisCount};
use crate::analyzer::python_path_utils::find_python_search_paths;
use crate::analyzer::service_utils::{
    find_config_file, find_config_file_here_or_up, find_pyproject_toml_file, find_pyproject_toml_file_here_or_up,
};
use crate::analyzer::source_enumerator::SourceEnumerator;
use crate::common::command_line_options::{CommandLineConfigOptions, CommandLineLanguageServerOptions, CommandLineOptions};
use crate::common::config_options::ConfigOptions;
use crate::common::console::{ConsoleInterface, LogLevel, log};
use crate::common::file_system::FileSystem;
use crate::common::host::Host;
use crate::common::path_consts::{DEFAULT_EXCLUDES, DEFAULT_STUBS_DIRECTORY};
use crate::common::path_utils::is_rooted_disk_path;
use crate::common::python_version::PythonVersion;
use crate::common::service_provider::ServiceProvider;
use crate::common::toml_utils;
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::{get_file_spec, is_directory};

// analyzer/service.ts:79
/// URI scheme prefixes like `memfs:/...`: two or more letters, then `:/`.
fn uri_scheme_regex() -> &'static Regex {
    thread_local! {
        static RE: &'static Regex =
            Box::leak(Box::new(Regex::new(r"^[a-zA-Z]{2,}[\w+.-]*://?").unwrap()));
    }
    RE.with(|r| *r)
}

// analyzer/service.ts:87
pub struct AnalyzerServiceOptions {
    pub console: &'static dyn ConsoleInterface,
    pub host_factory: Box<dyn Fn() -> &'static dyn Host>,
    pub max_analysis_time: Option<MaxAnalysisTime>,
    pub skip_scanning_user_files: bool,
}

// analyzer/service.ts:110
struct ConfigFileContents {
    config_file_dir_uri: Uri,
    config_file_json_obj: Value,
}

thread_local! {
    // analyzer/service.ts:116
    static NEXT_SERVICE_ID: Cell<u32> = const { Cell::new(1) };
}

// analyzer/service.ts:118
pub fn get_next_service_id(name: &str) -> String {
    let id = NEXT_SERVICE_ID.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    format!("{name}_{id}")
}

// analyzer/service.ts:122
pub struct AnalyzerService {
    options: AnalyzerServiceOptions,
    background_analysis_program: BackgroundAnalysisProgram,
    service_provider: ServiceProvider,
    instance_name: String,
    execution_root_uri: Cell<Uri>,
    primary_config_file_uri: Cell<Option<Uri>>,
    extended_config_file_uris: RefCell<Vec<Uri>>,
    command_line_options: RefCell<Option<CommandLineOptions>>,
    require_tracked_file_update: Cell<bool>,
    source_enumerator: RefCell<Option<SourceEnumerator>>,
    /// The pending `_analyzeTimer`.
    analysis_timer: Cell<bool>,
}

impl AnalyzerService {
    // analyzer/service.ts:149
    pub fn new(instance_name: &str, service_provider: ServiceProvider, options: AnalyzerServiceOptions) -> AnalyzerService {
        let _service_id = get_next_service_id(instance_name);

        // Override the console with the one the caller provided.
        let service_provider = ServiceProvider { console: options.console, ..service_provider };

        let config_options = P::new(ConfigOptions::new(Uri::file(
            &crate::common::uri::uri::process_cwd(),
            service_provider.case_sensitivity_detector,
            false,
        )));
        let import_resolver = AnalyzerService::create_import_resolver(service_provider, config_options, (options.host_factory)());

        let background_analysis_program = BackgroundAnalysisProgram::new(
            service_provider,
            config_options,
            import_resolver,
            options.max_analysis_time,
            false,
        );

        AnalyzerService {
            options,
            background_analysis_program,
            service_provider,
            instance_name: instance_name.to_string(),
            execution_root_uri: Cell::new(Uri::empty()),
            primary_config_file_uri: Cell::new(None),
            extended_config_file_uris: RefCell::new(Vec::new()),
            command_line_options: RefCell::new(None),
            require_tracked_file_update: Cell::new(true),
            source_enumerator: RefCell::new(None),
            analysis_timer: Cell::new(false),
        }
    }

    // analyzer/service.ts:201
    pub fn fs(&self) -> &'static dyn FileSystem {
        self.background_analysis_program.import_resolver().file_system()
    }

    // analyzer/service.ts:205
    pub fn service_provider(&self) -> ServiceProvider {
        self.service_provider
    }

    // analyzer/service.ts:217
    pub fn background_analysis_program(&self) -> &BackgroundAnalysisProgram {
        &self.background_analysis_program
    }

    // analyzer/service.ts:319
    pub fn create_import_resolver(
        service_provider: ServiceProvider,
        options: P<ConfigOptions>,
        host: &'static dyn Host,
    ) -> P<ImportResolver> {
        P::new(ImportResolver::new(service_provider, options, host))
    }

    // analyzer/service.ts:323
    pub fn set_completion_callback(&self, callback: Option<AnalysisCompleteCallback>) {
        self.background_analysis_program.set_completion_callback(callback);
    }

    // analyzer/service.ts:328
    pub fn set_options(&self, command_line_options: CommandLineOptions) {
        *self.command_line_options.borrow_mut() = Some(command_line_options.clone());

        let host = (self.options.host_factory)();
        let config_options = self.compute_config_options(host, &command_line_options);

        self.background_analysis_program.set_config_options(config_options);

        self.execution_root_uri.set(config_options.project_root);
        self.apply_config_options(host);
    }

    // analyzer/service.ts:458
    pub fn get_config_options(&self) -> P<ConfigOptions> {
        self.config_options()
    }

    // analyzer/service.ts:462
    pub fn get_import_resolver(&self) -> P<ImportResolver> {
        self.background_analysis_program.import_resolver()
    }

    // analyzer/service.ts:476
    pub fn test_get_config_options(&self, command_line_options: &CommandLineOptions) -> P<ConfigOptions> {
        self.compute_config_options(self.background_analysis_program.host(), command_line_options)
    }

    // analyzer/service.ts:480
    pub fn test_get_file_names_from_file_specs(&self) -> Vec<Uri> {
        let config_options = self.config_options();
        let mut enumerator = SourceEnumerator::new(
            &config_options.include,
            config_options.exclude.clone(),
            config_options.auto_exclude_venv.unwrap_or(false),
            self.fs(),
            self.console(),
        );

        let results = enumerator.enumerate(0.0);
        results.matches.values().copied().collect()
    }

    // analyzer/service.ts:528
    /// Attempts to make progress on source file enumeration. Returns true if complete.
    pub fn enumerate_source_files(&self, max_source_enumerator_time: f64) -> bool {
        // If there is no active source enumerator, we're done.
        if self.source_enumerator.borrow().is_none() {
            return true;
        }

        if self.execution_root_uri.get().is_empty() {
            // No user files for default workspace.
            return true;
        }

        let enum_results = self.source_enumerator.borrow_mut().as_mut().unwrap().enumerate(max_source_enumerator_time);

        if !enum_results.is_complete {
            return false;
        }

        // Pyright pushes the auto-excluded directories onto the exclude list
        // of the ConfigOptions the service was constructed with, which nothing
        // reads afterwards. Only the setConfigOptions call has an effect.
        self.background_analysis_program.set_config_options(self.config_options());

        let file_map = enum_results.matches;
        let files: Vec<Uri> = file_map.values().copied().collect();
        self.background_analysis_program.set_tracked_files(&files);

        // Source file enumeration is complete. Proceed with analysis.
        *self.source_enumerator.borrow_mut() = None;

        true
    }

    // analyzer/service.ts:582
    fn run_analysis(&self) {
        let more_to_analyze = self.background_analysis_program.start_analysis();
        if more_to_analyze {
            self.schedule_reanalysis(false);
        }
    }

    // analyzer/service.ts:597
    fn schedule_reanalysis(&self, require_tracked_file_update: bool) {
        let enable_ambient_analysis = self
            .command_line_options
            .borrow()
            .as_ref()
            .is_some_and(|o| o.language_server_settings.enable_ambient_analysis);
        if !enable_ambient_analysis {
            return;
        }

        if require_tracked_file_update {
            self.require_tracked_file_update.set(true);
        }

        // Replace any existing analysis timer.
        self.analysis_timer.set(true);
    }

    /// Fires pending timers until none is left, as Node's event loop does
    /// before the process exits.
    pub fn run_timers(&self) {
        while self.analysis_timer.replace(false) {
            self.on_analysis_timer();
        }
    }

    // analyzer/service.ts:597
    /// The body of the `setTimeout` callback in `scheduleReanalysis`.
    fn on_analysis_timer(&self) {
        if self.require_tracked_file_update.get() {
            self.update_tracked_file_list(false);
        }

        // Continue to enumerate sources if we haven't finished doing so.
        let max_source_enumerator_time = self.options.max_analysis_time.map_or(0.0, |t| t.no_open_files_time_in_ms);
        if !self.enumerate_source_files(max_source_enumerator_time) {
            self.schedule_reanalysis(false);
            return;
        }

        self.run_analysis();
    }

    // analyzer/service.ts:655
    fn apply_config_options(&self, host: &'static dyn Host) {
        // Allocate a new import resolver because the old one has information
        // cached based on the previous config options.
        let import_resolver = AnalyzerService::create_import_resolver(
            self.service_provider,
            self.background_analysis_program.config_options(),
            host,
        );

        self.background_analysis_program.set_import_resolver(import_resolver);

        let config_options = self.config_options();
        let from_language_server = self.command_line_options.borrow().as_ref().is_some_and(|o| o.from_language_server);
        if from_language_server || config_options.verbose_output.unwrap_or(false) {
            let log_level = if config_options.verbose_output.unwrap_or(false) { LogLevel::Info } else { LogLevel::Log };
            let console = self.console();

            for exec_env in config_options.get_execution_environments() {
                log(console, log_level, &format!("Execution environment: {}", exec_env.name));
                log(console, log_level, "  Extra paths:");
                if !exec_env.extra_paths.is_empty() {
                    for path in &exec_env.extra_paths {
                        log(console, log_level, &format!("    {}", path.to_user_visible_string()));
                    }
                } else {
                    log(console, log_level, "    (none)");
                }
                log(
                    console,
                    log_level,
                    &format!("  Python version: {}", PythonVersion::to_string(&exec_env.python_version)),
                );
                log(
                    console,
                    log_level,
                    &format!("  Python platform: {}", exec_env.python_platform.as_deref().unwrap_or("All")),
                );
                log(console, log_level, "  Search paths:");
                for path in import_resolver.get_import_roots(&exec_env, true) {
                    log(console, log_level, &format!("    {}", path.to_user_visible_string()));
                }
            }
        }

        self.update_tracked_file_list(true);

        self.schedule_reanalysis(false);
    }

    // analyzer/service.ts:703
    fn console(&self) -> &'static dyn ConsoleInterface {
        self.options.console
    }

    // analyzer/service.ts:715
    pub fn program(&self) -> P<Program> {
        self.background_analysis_program.program()
    }

    // analyzer/service.ts:719
    fn config_options(&self) -> P<ConfigOptions> {
        self.background_analysis_program.config_options()
    }

    // analyzer/service.ts:748
    fn compute_config_options(&self, host: &'static dyn Host, command_line_options: &CommandLineOptions) -> P<ConfigOptions> {
        let detector = self.service_provider.case_sensitivity_detector;
        let fs = self.fs();
        let execution_root_uri = match command_line_options.execution_root.as_deref() {
            Some(option_root) if !option_root.is_empty() => Uri::file(option_root, detector, true),
            _ => Uri::default_workspace(detector),
        };

        let execution_root = fs.real_case_path(execution_root_uri);
        let mut project_root = execution_root;
        let mut config_file_path: Option<Uri> = None;
        let mut pyproject_file_path: Option<Uri> = None;

        if let Some(cl_config_file_path) = command_line_options.config_file_path.as_deref().filter(|p| !p.is_empty()) {
            // If the config file path was specified, determine whether it's
            // a directory (in which case the default config file name is assumed)
            // or a file.
            let path = fs.real_case_path(if is_rooted_disk_path(cl_config_file_path) {
                Uri::file(cl_config_file_path, detector, true)
            } else {
                project_root.resolve_paths(&[cl_config_file_path])
            });

            if !fs.exists_sync(path) {
                self.console().info(&format!("Configuration file not found at {}.", path.to_user_visible_string()));
                config_file_path = Some(project_root);
            } else if path.last_extension().ends_with(".json") || path.last_extension().ends_with(".toml") {
                project_root = path.get_directory();
                config_file_path = Some(path);
            } else {
                project_root = path;
                config_file_path = find_config_file(fs, path);
                if config_file_path.is_none() {
                    self.console()
                        .info(&format!("Configuration file not found at {}.", project_root.to_user_visible_string()));
                }
            }
        } else if command_line_options.execution_root.as_deref().is_some_and(|r| !r.is_empty()) {
            // In a project-based IDE like VS Code, we should assume that the
            // project root directory contains the config file.
            config_file_path = find_config_file(fs, project_root);

            // If pyright is being executed from the command line, the working
            // directory may be deep within a project, and we need to walk up the
            // directory hierarchy to find the project root.
            if config_file_path.is_none() && !command_line_options.from_language_server {
                config_file_path = find_config_file_here_or_up(fs, project_root);
            }

            if let Some(path) = config_file_path {
                project_root = path.get_directory();
            } else {
                self.console().log("No configuration file found.");
            }
        }

        if config_file_path.is_none() {
            // See if we can find a pyproject.toml file in this directory.
            pyproject_file_path = find_pyproject_toml_file(fs, project_root);

            if pyproject_file_path.is_none() && !command_line_options.from_language_server {
                pyproject_file_path = find_pyproject_toml_file_here_or_up(fs, project_root);
            }

            if let Some(path) = pyproject_file_path {
                project_root = path.get_directory();
                self.console()
                    .log(&format!("pyproject.toml file found at {}.", project_root.to_user_visible_string()));
            } else {
                self.console().log("No pyproject.toml file found.");
            }
        }

        let mut config_options = ConfigOptions::new(project_root);

        // If we found a config file, load it and apply its settings.
        let configs = self.get_extended_configurations(
            config_file_path.or(pyproject_file_path),
            if config_file_path.is_none() { pyproject_file_path.map(|p| p.get_directory()) } else { None },
        );
        let console = self.service_provider.console;
        if let Some(configs) = configs.as_ref().filter(|c| !c.is_empty()) {
            // With a pyrightconfig.json set, we want the typeCheckingMode to always be standard
            // as that's what the Pyright CLI will expect.
            config_options.initialize_type_checking_mode(Some("standard"), None);

            // Then we apply the config file settings. This can update the
            // the typeCheckingMode.
            for config in configs {
                config_options.initialize_from_json(
                    &config.config_file_json_obj,
                    config.config_file_dir_uri,
                    console,
                    self.service_provider.fs,
                    host,
                );
            }

            // Set the configFileSource since we have a config file.
            config_options.config_file_source = self.primary_config_file_uri.get();

            // When not in language server mode, command line options override config file options.
            if !command_line_options.from_language_server {
                self.apply_command_line_overrides(
                    &mut config_options,
                    &command_line_options.config_settings,
                    project_root,
                    false,
                );
            }
        } else {
            // Initialize the type checking mode based on if this is for a language server or not.
            config_options.initialize_type_checking_mode(
                Some(if command_line_options.from_language_server { "off" } else { "standard" }),
                None,
            );

            // If there are no config files, we can then directly apply the command line options.
            self.apply_command_line_overrides(
                &mut config_options,
                &command_line_options.config_settings,
                project_root,
                command_line_options.from_language_server,
            );
        }

        // Apply the command line options that are not in the config file.
        self.apply_language_server_options(&mut config_options, project_root, &command_line_options.language_server_settings);

        // Ensure that if no command line or config options were applied, we have some defaults.
        self.ensure_default_options(host, &mut config_options, project_root, execution_root, command_line_options);

        // Once we have defaults, we can then setup the execution environments.
        if let Some(configs) = &configs {
            for config in configs {
                config_options.setup_execution_environments(
                    &config.config_file_json_obj,
                    config.config_file_dir_uri,
                    console,
                    fs,
                );
            }
        }

        P::new(config_options)
    }

    // analyzer/service.ts:889
    fn ensure_default_options(
        &self,
        host: &'static dyn Host,
        config_options: &mut ConfigOptions,
        project_root: Uri,
        execution_root: Uri,
        command_line_options: &CommandLineOptions,
    ) {
        let fs = self.fs();
        let console = self.console();

        // If no include paths were provided, assume that all files within
        // the project should be included.
        if config_options.include.is_empty() {
            console.info(&format!("No include entries specified; assuming {}", project_root.to_user_visible_string()));
            config_options.include.push(get_file_spec(project_root, "."));
        }

        config_options.user_specified_excludes = !config_options.exclude.is_empty();

        // Add the built-in default excludes unless the user turned them off.
        let use_default_excludes = command_line_options.config_settings.use_default_excludes.unwrap_or(true);
        if use_default_excludes {
            // Deduplicate against patterns the user already listed (by compiled regex source).
            let mut existing_exclude_reg_exps: JsSet<String> =
                config_options.exclude.iter().map(|spec| spec.reg_exp.as_str().to_string()).collect();
            for exclude in DEFAULT_EXCLUDES {
                let file_spec = get_file_spec(project_root, exclude);
                if existing_exclude_reg_exps.contains(file_spec.reg_exp.as_str()) {
                    continue;
                }

                console.info(&format!("Auto-excluding {exclude}"));
                existing_exclude_reg_exps.insert(file_spec.reg_exp.as_str().to_string());
                config_options.exclude.push(file_spec);
            }
        }

        config_options.auto_exclude_venv = Some(use_default_excludes);

        if config_options.default_extra_paths.is_none() {
            config_options.ensure_default_extra_paths(
                fs,
                command_line_options.config_settings.auto_search_paths.unwrap_or(false),
                command_line_options.config_settings.extra_paths.as_deref(),
            );
        }

        if config_options.default_python_platform.is_none() {
            config_options.default_python_platform = command_line_options.config_settings.python_platform.clone();
        }
        if config_options.default_python_version.is_none() {
            config_options.default_python_version = command_line_options.config_settings.python_version;
        }

        // If the caller specified that "typeshedPath" is the root of the project,
        // then we're presumably running in the typeshed project itself. Auto-exclude
        // stdlib packages that don't match the current Python version.
        if let (Some(typeshed_path), Some(default_python_version)) =
            (config_options.typeshed_path, config_options.default_python_version)
        {
            if Uri::ptr_eq(typeshed_path, project_root) {
                let exclude_list = self.get_import_resolver().get_typeshed_stdlib_exclude_list(
                    Some(typeshed_path),
                    default_python_version,
                    config_options.default_python_platform.as_deref(),
                );

                console.info("Excluding typeshed stdlib stubs according to VERSIONS file:");
                for exclude in exclude_list {
                    console.info(&format!("    {}", exclude.to_uri_string()));
                    config_options.exclude.push(get_file_spec(execution_root, exclude.get_file_path()));
                }
            }
        }

        // If useLibraryCodeForTypes is unspecified, default it to true.
        if config_options.use_library_code_for_types.is_none() {
            config_options.use_library_code_for_types = Some(true);
        }
        if let Some(stub_path) = config_options.stub_path {
            // If there was a stub path specified, validate it.
            if !fs.exists_sync(stub_path) || !is_directory(fs, stub_path) {
                console.warn(&format!("stubPath {} is not a valid directory.", stub_path.to_uri_string()));
            }
        } else {
            // If no stub path was specified, use a default path.
            config_options.stub_path = Some(config_options.project_root.resolve_paths(&[DEFAULT_STUBS_DIRECTORY]));
        }

        // Do some sanity checks on the specified settings and report missing
        // or inconsistent information.
        if let Some(venv_path) = config_options.venv_path {
            if !fs.exists_sync(venv_path) || !is_directory(fs, venv_path) {
                console.error(&format!("venvPath {} is not a valid directory.", venv_path.to_user_visible_string()));
            }

            // venvPath without venv means it won't do anything while resolveImport.
            config_options.venv = config_options.venv.clone().or_else(|| self.config_options().venv.clone());
            if let Some(venv) = config_options.venv.clone() {
                let full_venv_path = venv_path.resolve_paths(&[&venv]);

                if !fs.exists_sync(full_venv_path) || !is_directory(fs, full_venv_path) {
                    console.error(&format!(
                        "venv {venv} subdirectory not found in venv path {}.",
                        venv_path.to_user_visible_string()
                    ));
                } else {
                    // `findPythonSearchPaths` always returns an array, so its
                    // `=== undefined` failure branch never runs.
                    let import_logger = config_options.verbose_output.unwrap_or(false).then(ImportLogger::new);
                    find_python_search_paths(fs, config_options, host, import_logger.as_ref(), false, None);
                }
            }
        }

        // Is there a reference to a venv? If so, there needs to be a valid venvPath.
        if config_options.venv.as_deref().is_some_and(|v| !v.is_empty()) && config_options.venv_path.is_none() {
            console.warn("venvPath not specified, so venv settings will be ignored.");
        }

        if let Some(typeshed_path) = config_options.typeshed_path {
            if !fs.exists_sync(typeshed_path) || !is_directory(fs, typeshed_path) {
                console.error(&format!(
                    "typeshedPath {} is not a valid directory.",
                    typeshed_path.to_user_visible_string()
                ));
            }
        }

        // If set on the command line, verboseOutput always overrides the config file.
        if let Some(verbose_output) = command_line_options.config_settings.verbose_output {
            config_options.verbose_output = Some(verbose_output);
        }

        // Ensure default python version and platform.
        config_options.ensure_default_python_version(host, console);
        config_options.ensure_default_python_platform(host, console);
    }

    // analyzer/service.ts:1051
    fn apply_language_server_options(
        &self,
        config_options: &mut ConfigOptions,
        project_root: Uri,
        language_server_options: &CommandLineLanguageServerOptions,
    ) {
        config_options.disable_tagged_hints = language_server_options.disable_tagged_hints.unwrap_or(false);
        if let Some(check_only_open_files) = language_server_options.check_only_open_files {
            config_options.check_only_open_files = Some(check_only_open_files);
        }
        if let Some(auto_import_completions) = language_server_options.auto_import_completions {
            config_options.auto_import_completions = auto_import_completions;
        }
        if let Some(indexing) = language_server_options.indexing {
            config_options.indexing = indexing;
        }
        if let Some(task_list_tokens) = &language_server_options.task_list_tokens {
            config_options.task_list_tokens = Some(task_list_tokens.clone());
        }
        config_options.log_type_evaluation_time = language_server_options.log_type_evaluation_time;
        config_options.type_evaluation_time_threshold = language_server_options.type_evaluation_time_threshold;

        // Special case, the language service can also set a pythonPath. It should override any other setting.
        if let Some(python_path) = language_server_options.python_path.as_deref().filter(|p| !p.is_empty()) {
            self.console()
                .info(&format!("Setting pythonPath for service \"{}\": \"{python_path}\"", self.instance_name));
            config_options.python_path = Some(
                self.fs().real_case_path(Uri::file(python_path, self.service_provider.case_sensitivity_detector, true)),
            );
        }
        if let Some(venv_path) = language_server_options.venv_path.as_deref().filter(|p| !p.is_empty()) {
            if config_options.venv_path.is_none() {
                config_options.venv_path = Some(project_root.resolve_paths(&[venv_path]));
            }
        }
    }

    // analyzer/service.ts:1090
    fn apply_command_line_overrides(
        &self,
        config_options: &mut ConfigOptions,
        command_line_options: &CommandLineConfigOptions,
        project_root: Uri,
        from_language_server: bool,
    ) {
        let fs = self.fs();
        let console = self.console();
        let detector = self.service_provider.case_sensitivity_detector;

        if let Some(type_checking_mode) = command_line_options.type_checking_mode.as_deref().filter(|m| !m.is_empty()) {
            config_options.initialize_type_checking_mode(Some(type_checking_mode), None);
        }

        if let Some(extra_paths) = &command_line_options.extra_paths {
            config_options.ensure_default_extra_paths(
                fs,
                command_line_options.auto_search_paths.unwrap_or(false),
                Some(extra_paths),
            );
        }

        if command_line_options.python_version.is_some()
            || command_line_options.python_platform.as_deref().is_some_and(|p| !p.is_empty())
        {
            config_options.default_python_version =
                command_line_options.python_version.or(config_options.default_python_version);
            config_options.default_python_platform =
                command_line_options.python_platform.clone().or_else(|| config_options.default_python_platform.clone());
        }

        if let Some(python_path) = command_line_options.python_path.as_deref().filter(|p| !p.is_empty()) {
            console.info(&format!("Setting pythonPath for service \"{}\": \"{python_path}\"", self.instance_name));
            config_options.python_path = Some(fs.real_case_path(Uri::file(python_path, detector, true)));
        }

        if let Some(name) = command_line_options.python_environment_name.as_deref().filter(|n| !n.is_empty()) {
            console.info(&format!("Setting environmentName for service \"{}\": \"{name}\"", self.instance_name));
            config_options.python_environment_name = Some(name.to_string());
        }

        for file_spec in &command_line_options.include_file_specs {
            // Skip file specs that look like URI schemes (e.g., "zowe-uss:", "memfs:").
            if uri_scheme_regex().is_match(file_spec) {
                continue;
            }
            config_options.include.push(get_file_spec(project_root, file_spec));
        }

        for file_spec in &command_line_options.exclude_file_specs {
            if uri_scheme_regex().is_match(file_spec) {
                continue;
            }
            config_options.exclude.push(get_file_spec(project_root, file_spec));
        }

        for file_spec in &command_line_options.ignore_file_specs {
            if uri_scheme_regex().is_match(file_spec) {
                continue;
            }
            config_options.ignore.push(get_file_spec(project_root, file_spec));
        }

        config_options.apply_diagnostic_overrides(command_line_options.diagnostic_severity_overrides.as_ref());
        config_options.apply_diagnostic_overrides(command_line_options.diagnostic_boolean_overrides.as_ref());

        // Override the analyzeUnannotatedFunctions setting based on the command-line setting.
        if let Some(analyze_unannotated_functions) = command_line_options.analyze_unannotated_functions {
            config_options.diagnostic_rule_set.analyze_unannotated_functions = analyze_unannotated_functions;
        }

        // Override the include based on command-line settings.
        if let Some(includes) = &command_line_options.include_file_specs_override {
            config_options.include = Vec::new();
            for include in includes {
                // Skip overrides that look like URI schemes (e.g., "memfs:/", "zowe-uss://").
                if uri_scheme_regex().is_match(include) {
                    continue;
                }
                config_options.include.push(get_file_spec(Uri::file(include, detector, true), "."));
            }
        }

        // Override the venvPath based on the command-line setting.
        if let Some(venv_path) = command_line_options.venv_path.as_deref().filter(|p| !p.is_empty()) {
            config_options.venv_path = Some(project_root.resolve_paths(&[venv_path]));
        }

        let report_duplicate_setting = |setting_name: &str, config_value: &str| {
            let setting_source = if from_language_server { "the client settings" } else { "a command-line option" };
            console.warn(&format!(
                "The {setting_name} has been specified in both the config file and {setting_source}. \
                 The value in the config file ({config_value}) will take precedence"
            ));
        };

        // Apply the command-line options if the corresponding
        // item wasn't already set in the config file. Report any
        // duplicates.
        if let Some(typeshed_path) = command_line_options.typeshed_path.as_deref().filter(|p| !p.is_empty()) {
            match config_options.typeshed_path {
                None => config_options.typeshed_path = Some(project_root.resolve_paths(&[typeshed_path])),
                Some(existing) => report_duplicate_setting("typeshedPath", existing.to_user_visible_string()),
            }
        }

        // If useLibraryCodeForTypes was not specified in the config, allow the command line to override it.
        match config_options.use_library_code_for_types {
            None => config_options.use_library_code_for_types = command_line_options.use_library_code_for_types,
            Some(existing) => {
                if command_line_options.use_library_code_for_types.is_some() {
                    report_duplicate_setting("useLibraryCodeForTypes", if existing { "true" } else { "false" });
                }
            }
        }

        if let Some(stub_path) = command_line_options.stub_path.as_deref().filter(|p| !p.is_empty()) {
            match config_options.stub_path {
                None => config_options.stub_path = Some(fs.real_case_path(project_root.resolve_paths(&[stub_path]))),
                Some(existing) => report_duplicate_setting("stubPath", existing.to_user_visible_string()),
            }
        }
    }

    // analyzer/service.ts:1225
    /// Loads the config JSON object from the specified config file along with
    /// any chained config files named by "extends" (recursively).
    fn get_extended_configurations(
        &self,
        primary_config_file_uri: Option<Uri>,
        pyproject_search_dir: Option<Uri>,
    ) -> Option<Vec<ConfigFileContents>> {
        self.primary_config_file_uri.set(primary_config_file_uri);
        *self.extended_config_file_uris.borrow_mut() = Vec::new();

        let primary_config_file_uri = primary_config_file_uri?;
        let mut cur_config_file_uri = primary_config_file_uri;

        let mut config_json_objs: Vec<ConfigFileContents> = Vec::new();

        loop {
            self.extended_config_file_uris.borrow_mut().push(cur_config_file_uri);

            // Is this a TOML or JSON file?
            let config_file_json_obj = if cur_config_file_uri.last_extension().ends_with(".toml") {
                self.console()
                    .info(&format!("Loading pyproject.toml file at {}", cur_config_file_uri.to_user_visible_string()));
                self.parse_pyproject_toml_file(cur_config_file_uri)
            } else {
                self.console()
                    .info(&format!("Loading configuration file at {}", cur_config_file_uri.to_user_visible_string()));
                self.parse_json_config_file(cur_config_file_uri)
            };

            let Some(config_file_json_obj) = config_file_json_obj.filter(is_truthy) else {
                break;
            };

            let config_dir = cur_config_file_uri.get_directory();
            let base_config_uri = ConfigOptions::resolve_extends(&config_file_json_obj, config_dir);

            // Push onto the start of the array so base configs are processed first.
            config_json_objs.insert(
                0,
                ConfigFileContents { config_file_json_obj, config_file_dir_uri: config_dir },
            );

            let Some(base_config_uri) = base_config_uri else {
                break;
            };

            // Check for circular references.
            if self.extended_config_file_uris.borrow().iter().any(|uri| uri.equals(Some(base_config_uri))) {
                self.console().error(&format!(
                    "Circular reference in configuration file \"extends\" setting: {} extends {}",
                    cur_config_file_uri.to_user_visible_string(),
                    base_config_uri.to_user_visible_string()
                ));
                break;
            }

            cur_config_file_uri = base_config_uri;
        }

        // If a pyproject.toml was found but had no [tool.pyright] section, fall back to
        // searching ancestor directories as if that pyproject.toml didn't exist.
        if config_json_objs.is_empty() {
            if let Some(pyproject_search_dir) = pyproject_search_dir {
                let parent_dir = pyproject_search_dir.get_directory();
                if !parent_dir.equals(Some(pyproject_search_dir)) {
                    let fallback = find_config_file_here_or_up(self.fs(), parent_dir)
                        .or_else(|| find_pyproject_toml_file_here_or_up(self.fs(), parent_dir));
                    if let Some(fallback) = fallback {
                        return self.get_extended_configurations(
                            Some(fallback),
                            // Provide the next pyprojectSearchDir, so we can continue
                            // searching upward.
                            if fallback.last_extension().ends_with(".toml") { Some(fallback.get_directory()) } else { None },
                        );
                    }
                }
            }
        }

        Some(config_json_objs)
    }

    // analyzer/service.ts:1299
    fn parse_json_config_file(&self, config_path: Uri) -> Option<Value> {
        self.attempt_parse_file(config_path, |file_contents, _| {
            // `JSONC.parse(..., { allowTrailingComma: true })`; any error throws.
            jsrt::jsonc::parse(file_contents).map(Some).map_err(|_| "Errors parsing JSON file".to_string())
        })
    }

    // analyzer/service.ts:1311
    fn parse_pyproject_toml_file(&self, pyproject_path: Uri) -> Option<Value> {
        self.attempt_parse_file(pyproject_path, |file_contents, attempt_count| {
            let config_obj = match toml_utils::parse(file_contents) {
                Ok(config_obj) => config_obj,
                Err(e) => {
                    // Pyright prints `JSON.stringify(e)` of smol-toml's TomlError.
                    self.console().error(&format!("Pyproject file parse attempt {attempt_count} error: {e}"));
                    return Err(e);
                }
            };

            if let Some(tool) = config_obj.get("tool") {
                // `configObj.tool.pyright`: undefined unless `tool` holds an object with it.
                return Ok(match tool {
                    Value::Object(map) => map.get("pyright").cloned(),
                    _ => None,
                });
            }

            self.console().info(&format!(
                "Pyproject file \"{}\" has no \"[tool.pyright]\" section.",
                pyproject_path.to_user_visible_string()
            ));
            Ok(None)
        })
    }

    // analyzer/service.ts:1330
    fn attempt_parse_file(
        &self,
        file_uri: Uri,
        parse_callback: impl Fn(&str, u32) -> Result<Option<Value>, String>,
    ) -> Option<Value> {
        let mut parse_attempt_count = 0;

        loop {
            // Attempt to read the file contents.
            let Ok(file_contents) = self.fs().read_file_text_sync(file_uri) else {
                self.console()
                    .error(&format!("Config file \"{}\" could not be read.", file_uri.to_user_visible_string()));
                self.report_config_parse_error();
                return None;
            };

            // Attempt to parse the file.
            if let Ok(result) = parse_callback(&file_contents, parse_attempt_count + 1) {
                return result;
            }

            // Pyright retries in case the file was read while partly written.
            let attempt = parse_attempt_count;
            parse_attempt_count += 1;
            if attempt >= 5 {
                self.console().error(&format!(
                    "Config file \"{}\" could not be parsed. Verify that format is correct.",
                    file_uri.to_user_visible_string()
                ));
                self.report_config_parse_error();
                return None;
            }
        }
    }

    // analyzer/service.ts:1379
    fn update_tracked_file_list(&self, mark_files_dirty_unconditionally: bool) {
        if !self.options.skip_scanning_user_files {
            // Allocate a new source enumerator. We'll call this
            // repeatedly until all source files are found.
            let config_options = self.config_options();
            *self.source_enumerator.borrow_mut() = Some(SourceEnumerator::new(
                &config_options.include,
                config_options.exclude.clone(),
                config_options.auto_exclude_venv.unwrap_or(false),
                self.fs(),
                self.console(),
            ));

            self.background_analysis_program.mark_all_files_dirty(mark_files_dirty_unconditionally);
            self.require_tracked_file_update.set(false);
        }
    }

    // analyzer/service.ts:1893
    fn report_config_parse_error(&self) {
        self.background_analysis_program.call_completion_callback(AnalysisResults {
            diagnostics: Vec::new(),
            files_in_program: 0,
            requiring_analysis_count: RequiringAnalysisCount { files: 0, cells: 0 },
            checking_only_open_files: true,
            fatal_error_occurred: false,
            config_parse_error_occurred: true,
            elapsed_time: 0.0,
            reason: AnalysisReason::Analysis,
        });
    }
}

/// JavaScript truthiness of a parsed config value.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}
