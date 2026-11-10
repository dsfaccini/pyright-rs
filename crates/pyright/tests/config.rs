//! Port of `packages/pyright-internal/src/tests/config.test.ts`.
//!
//! The TS tests run with the jest cwd `packages/pyright-internal` and read the
//! sample projects under `src/tests/samples/` relative to it. Here `cwd()`
//! builds that directory from `$HOME/pyright`, the pinned pyright checkout, so
//! the result does not depend on the test process cwd.
//!
//! `createAnalyzer` uses `tests/harness/testAccessHost.ts`. The lib's port of
//! it is `#[cfg(test)]` and unreachable from an integration test, so
//! `TestAccessHost` below ports it again, with the `getPythonVersion`
//! override `createAnalyzer` installs.

use jsrt::P;
use pyright::analyzer::import_logger::ImportLogger;
use pyright::analyzer::python_path_utils::PythonPathResult;
use pyright::analyzer::service::{AnalyzerService, AnalyzerServiceOptions};
use pyright::common::case_sensitivity_detector::CaseSensitiveDetector;
use pyright::common::command_line_options::{CommandLineOptions, DiagnosticSeverityOverridesMap};
use pyright::common::config_options::{
    ConfigOptions, DiagnosticLevel, DiagnosticOverride, ExecutionEnvironment, PythonPlatform,
    get_basic_diagnostic_rule_set, get_standard_diagnostic_rule_set,
};
use pyright::common::console::NullConsole;
use pyright::common::diagnostic::{TaskListPriority, TaskListToken};
use pyright::common::host::{Host, HostKind};
use pyright::common::path_utils::{combine_paths, normalize_path, normalize_slashes};
use pyright::common::python_version::{PYTHON_VERSION3_9, PYTHON_VERSION3_13, PythonVersion};
use pyright::common::real_file_system::{RealTempFile, create_from_real_file_system};
use pyright::common::service_provider::ServiceProvider;
use pyright::common::uri::Uri;
use pyright::common::uri::uri_utils::FileSpec;

// tests/harness/testAccessHost.ts:16
/// `extends NoAccessHost`. `python_version` is the `host.getPythonVersion`
/// override; `None` keeps `NoAccessHost.getPythonVersion`.
struct TestAccessHost {
    module_path: Uri,
    search_paths: Vec<Uri>,
    python_version: Option<PythonVersion>,
}

impl Host for TestAccessHost {
    fn kind(&self) -> HostKind {
        HostKind::NoAccess
    }

    // tests/harness/testAccessHost.ts:21
    fn get_python_search_paths(
        &self,
        _python_path: Option<Uri>,
        _import_logger: Option<&ImportLogger>,
        _cwd: Option<Uri>,
    ) -> PythonPathResult {
        // No file system is passed, so no path is filtered out.
        PythonPathResult {
            paths: self.search_paths.clone(),
            prefix: Some(self.module_path),
        }
    }

    fn get_python_version(
        &self,
        _python_path: Option<Uri>,
        _import_logger: Option<&ImportLogger>,
    ) -> Option<PythonVersion> {
        self.python_version
    }

    fn get_python_platform(&self, _import_logger: Option<&ImportLogger>) -> Option<PythonPlatform> {
        None
    }
}

fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

thread_local! {
    // tests/config.test.ts:34
    static TEMP_FILE: &'static RealTempFile = leak(RealTempFile::new());
}

fn temp_file() -> &'static RealTempFile {
    TEMP_FILE.with(|t| *t)
}

/// `normalizePath(process.cwd())` under jest: `packages/pyright-internal`.
fn cwd() -> String {
    let home = std::env::var("HOME").expect("HOME is set");
    normalize_path(&format!("{home}/pyright/packages/pyright-internal"))
}

/// `normalizePath(combinePaths(process.cwd(), sample))`.
fn sample_dir(sample: &str) -> String {
    normalize_path(&combine_paths(&cwd(), &[sample]))
}

// tests/config.test.ts:942
fn create_analyzer(console: Option<&'static NullConsole>) -> AnalyzerService {
    let cons = console.unwrap_or_else(|| leak(NullConsole::default()));
    let fs = leak(create_from_real_file_system(temp_file(), cons));
    let service_provider = ServiceProvider::new(fs, cons, temp_file());
    let host: &'static TestAccessHost = leak(TestAccessHost {
        module_path: Uri::empty(),
        search_paths: Vec::new(),
        python_version: Some(PYTHON_VERSION3_13),
    });
    AnalyzerService::new(
        "<default>",
        service_provider,
        AnalyzerServiceOptions {
            console: cons,
            host_factory: Box::new(move || -> &'static dyn Host { host }),
            max_analysis_time: None,
            skip_scanning_user_files: false,
        },
    )
}

/// `Uri.file(path, service.serviceProvider)`.
fn service_file(service: &AnalyzerService, path: &str) -> Uri {
    Uri::file(
        path,
        service.service_provider().case_sensitivity_detector,
        false,
    )
}

/// `UriEx.file(path)`.
fn uri_ex_file(path: &str) -> Uri {
    Uri::file(path, &CaseSensitiveDetector, false)
}

fn file_names(uris: &[Uri]) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = uris.iter().map(|p| p.file_name()).collect();
    names.sort();
    names
}

fn reg_exp_sources(specs: &[FileSpec]) -> Vec<&str> {
    specs.iter().map(|s| s.reg_exp.as_str()).collect()
}

/// The fields `assert.deepStrictEqual` compares on a `FileSpec`.
fn file_spec_values(specs: &[FileSpec]) -> Vec<(&'static str, &str, bool)> {
    specs
        .iter()
        .map(|s| {
            (
                s.wildcard_root.key(),
                s.reg_exp.as_str(),
                s.has_directory_wildcard,
            )
        })
        .collect()
}

/// `assert.deepStrictEqual` on a `ConfigOptions`: every field by value. An
/// `ExecutionEnvironment` handle prints its address, so environments are
/// compared by content.
fn config_options_values(options: &ConfigOptions) -> String {
    let envs: Vec<&ExecutionEnvironment> = options
        .execution_environments
        .iter()
        .map(|e| e.get())
        .collect();
    let mut rest = options.clone();
    rest.execution_environments.clear();
    format!("{rest:?} {envs:?}")
}

// tests/config.test.ts:38
#[test]
fn find_files_with_config_file() {
    let cwd = cwd();
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd.clone()), true);
    command_line_options.config_file_path = Some("src/tests/samples/project1".to_string());

    let config_options = service.test_get_config_options(&command_line_options);
    service.set_options(command_line_options.clone());

    // The config file specifies a single file spec (a directory).
    assert_eq!(
        config_options.include.len(),
        1,
        "failed creating options from {cwd}"
    );
    assert_eq!(
        config_options.project_root.key(),
        service
            .fs()
            .real_case_path(service_file(
                &service,
                &combine_paths(
                    &cwd,
                    &[command_line_options.config_file_path.as_deref().unwrap()]
                )
            ))
            .key()
    );

    let file_list = service.test_get_file_names_from_file_specs();

    // The config file specifies a subdirectory, so we should find
    // only two of the three "*.py" files present in the project
    // directory.
    assert_eq!(file_list.len(), 2);
}

// tests/config.test.ts:64
#[test]
fn find_files_virtual_env_auto_detect_exclude() {
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd()), true);
    command_line_options.config_file_path =
        Some("src/tests/samples/project_with_venv_auto_detect_exclude".to_string());

    service.set_options(command_line_options);

    // The config file is empty, so no 'exclude' are specified
    // The myVenv directory is detected as a venv and will be automatically excluded
    let file_list = service.test_get_file_names_from_file_specs();

    // There are 3 python files in the workspace, outside of myVenv
    // There is 1 python file in myVenv, which should be excluded
    assert_eq!(
        file_names(&file_list),
        ["sample1.py", "sample2.py", "sample3.py"]
    );
}

// tests/config.test.ts:82
#[test]
#[ignore = "needs the in-memory vfs TestFileSystem, which is #[cfg(test)] inside the lib"]
fn find_files_excludes_editable_install_shadow() {
    panic!("vfs.TestFileSystem is not available to integration tests");
}

// tests/config.test.ts:126
#[test]
#[ignore = "needs the in-memory vfs TestFileSystem, which is #[cfg(test)] inside the lib"]
fn editable_install_shadow_excluded_but_importable() {
    panic!("vfs.TestFileSystem is not available to integration tests");
}

// tests/config.test.ts:188
#[test]
fn find_files_virtual_env_auto_detect_with_user_exclude() {
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd()), true);
    command_line_options.config_file_path =
        Some("src/tests/samples/project_with_venv_auto_detect_include".to_string());

    service.set_options(command_line_options);

    // The config file defines an 'exclude' folder. Virtual env auto-detection remains
    // enabled even when the user specifies custom excludes (the excludes are additive),
    // so myVenv is still auto-excluded.
    let file_list = service.test_get_file_names_from_file_specs();

    // There are 3 python files in the workspace, outside of myVenv
    // There is 1 more python file in the user-excluded folder (excluded)
    // There is 1 python file in myVenv, which is auto-excluded
    assert_eq!(
        file_names(&file_list),
        ["sample1.py", "sample2.py", "sample3.py"]
    );
}

// tests/config.test.ts:208
#[test]
fn find_files_virtual_env_not_auto_detected_when_default_excludes_disabled() {
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd()), true);
    command_line_options.config_file_path =
        Some("src/tests/samples/project_with_venv_auto_detect_include".to_string());
    // Turn off the built-in default excludes; virtual-environment auto-detection is disabled
    // along with them, so myVenv is scanned again.
    command_line_options.config_settings.use_default_excludes = Some(false);

    service.set_options(command_line_options);

    let file_list = service.test_get_file_names_from_file_specs();

    // myVenv is no longer auto-excluded, so library1.py is scanned. The user-specified
    // 'exclude' folder is still excluded (explicit user excludes are unaffected by the setting).
    assert_eq!(
        file_names(&file_list),
        ["library1.py", "sample1.py", "sample2.py", "sample3.py"]
    );
}

// tests/config.test.ts:227
#[test]
fn file_spec_not_an_array() {
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let mut command_line_options = CommandLineOptions::new(Some(cwd()), false);
    command_line_options.config_file_path = Some("src/tests/samples/project2".to_string());
    service.set_options(command_line_options.clone());

    service.test_get_config_options(&command_line_options);

    // The method should return a default config and log an error.
    assert!(null_console.info_count.get() > 0);
}

// tests/config.test.ts:241
#[test]
fn file_spec_not_a_string() {
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let mut command_line_options = CommandLineOptions::new(Some(cwd()), false);
    command_line_options.config_file_path = Some("src/tests/samples/project3".to_string());
    service.set_options(command_line_options.clone());

    service.test_get_config_options(&command_line_options);

    // The method should return a default config and log an error.
    assert!(null_console.info_count.get() > 0);
}

// tests/config.test.ts:255
#[test]
fn some_file_specs_are_invalid() {
    let cwd = cwd();
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let mut command_line_options = CommandLineOptions::new(Some(cwd.clone()), false);
    command_line_options.config_file_path = Some("src/tests/samples/project4".to_string());
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);

    // The config file specifies four file specs in the include array
    // and one in the exclude array. The 4 default excludes are always applied
    // additively, so the exclude array contains 1 user + 4 defaults = 5.
    assert_eq!(
        config_options.include.len(),
        4,
        "failed creating options from {cwd}"
    );
    assert_eq!(config_options.exclude.len(), 5);
    assert_eq!(
        config_options.project_root.get_file_path(),
        service
            .fs()
            .real_case_path(service_file(
                &service,
                &combine_paths(
                    &cwd,
                    &[command_line_options.config_file_path.as_deref().unwrap()]
                )
            ))
            .get_file_path()
    );

    let file_list = service.test_get_file_names_from_file_specs();

    // We should receive two final files that match the include/exclude rules.
    assert_eq!(file_list.len(), 2);
}

// tests/config.test.ts:283
#[test]
fn default_excludes_disabled_by_use_default_excludes() {
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let mut command_line_options = CommandLineOptions::new(Some(cwd()), false);
    command_line_options.config_file_path = Some("src/tests/samples/project4".to_string());
    // Turn off the built-in default excludes.
    command_line_options.config_settings.use_default_excludes = Some(false);
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);

    // Only the user's single exclude remains; none of the 4 default excludes are added.
    assert_eq!(config_options.exclude.len(), 1);
    // Virtual-environment auto-detection is disabled along with the default excludes.
    assert_eq!(config_options.auto_exclude_venv, Some(false));
}

// tests/config.test.ts:301
#[test]
fn config_bad_json() {
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let mut command_line_options = CommandLineOptions::new(Some(cwd()), false);
    command_line_options.config_file_path = Some("src/tests/samples/project5".to_string());
    service.set_options(command_line_options.clone());

    service.test_get_config_options(&command_line_options);

    // The method should return a default config and log an error.
    assert!(null_console.info_count.get() > 0);
}

// tests/config.test.ts:315
#[test]
fn find_exec_env1() {
    let cwd = uri_ex_file(&cwd());
    let mut config_options = ConfigOptions::new(cwd);

    // Build a config option with three execution environments.
    let exec_env1 = P::new(ExecutionEnvironment::new(
        "python".to_string(),
        cwd.resolve_paths(&["src/foo"]),
        &get_standard_diagnostic_rule_set(),
        /* defaultPythonVersion */ None,
        /* defaultPythonPlatform */ None,
        /* defaultExtraPaths */ None,
        false,
    ));
    config_options.execution_environments.push(exec_env1);
    let exec_env2 = P::new(ExecutionEnvironment::new(
        "python".to_string(),
        cwd.resolve_paths(&["src"]),
        &get_standard_diagnostic_rule_set(),
        /* defaultPythonVersion */ None,
        /* defaultPythonPlatform */ None,
        /* defaultExtraPaths */ None,
        false,
    ));
    config_options.execution_environments.push(exec_env2);

    let file1 = cwd.resolve_paths(&["src/foo/bar.py"]);
    assert_eq!(config_options.find_exec_environment(file1), exec_env1);
    let file2 = cwd.resolve_paths(&["src/foo2/bar.py"]);
    assert_eq!(config_options.find_exec_environment(file2), exec_env2);

    // If none of the execution environments matched, we should get
    // a default environment with the root equal to that of the config.
    let file4 = uri_ex_file("/nothing/bar.py");
    let default_exec_env = config_options.find_exec_environment(file4);
    assert!(default_exec_env.root.is_some());
    let root_file_path = default_exec_env.root.unwrap().get_file_path();
    assert_eq!(
        normalize_slashes(root_file_path),
        normalize_slashes(config_options.project_root.get_file_path())
    );
}

// tests/config.test.ts:353
#[test]
fn python_platform() {
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let cwd = service_file(
        &service,
        &sample_dir("src/tests/samples/project_with_pyproject_toml_platform"),
    );
    let command_line_options =
        CommandLineOptions::new(Some(cwd.get_file_path().to_string()), false);
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);
    assert!(!config_options.execution_environments.is_empty());
    assert_eq!(
        config_options.execution_environments[0]
            .python_platform
            .as_deref(),
        Some("platform")
    );
}

// tests/config.test.ts:368
#[test]
fn auto_search_paths_on() {
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let cwd = service_file(&service, &sample_dir("src/tests/samples/project_src"));
    let mut command_line_options =
        CommandLineOptions::new(Some(cwd.get_file_path().to_string()), false);
    command_line_options.config_settings.auto_search_paths = Some(true);
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);

    let expected_extra_paths = vec![service.fs().real_case_path(cwd.combine_paths(&["src"]))];
    assert_eq!(
        config_options.default_extra_paths,
        Some(expected_extra_paths)
    );
}

// tests/config.test.ts:385
#[test]
fn auto_search_paths_off() {
    let cwd = sample_dir("src/tests/samples/project_src");
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let mut command_line_options = CommandLineOptions::new(Some(cwd), false);
    command_line_options.config_settings.auto_search_paths = Some(false);
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);

    assert!(config_options.execution_environments.is_empty());
}

// tests/config.test.ts:398
#[test]
fn auto_search_paths_on_src_is_pkg() {
    let cwd = sample_dir("src/tests/samples/project_src_is_pkg");
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let mut command_line_options = CommandLineOptions::new(Some(cwd), false);
    command_line_options.config_settings.auto_search_paths = Some(true);
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);

    // The src folder is a package (has __init__.py) and so should not be automatically added as extra path
    assert!(config_options.execution_environments.is_empty());
}

// tests/config.test.ts:412
#[test]
fn auto_search_paths_on_with_config_exec_env() {
    let cwd = sample_dir("src/tests/samples/project_src_with_config_extra_paths");
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let mut command_line_options = CommandLineOptions::new(Some(cwd.clone()), false);
    command_line_options.config_file_path = Some(combine_paths(&cwd, &["pyrightconfig.json"]));
    command_line_options.config_settings.auto_search_paths = Some(true);
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);

    // The extraPaths in the config file should override the setting.
    let expected_extra_paths: Vec<Uri> = Vec::new();

    assert_eq!(
        config_options.default_extra_paths,
        Some(expected_extra_paths)
    );
}

// tests/config.test.ts:429
#[test]
fn auto_search_paths_on_and_extra_paths() {
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let cwd = service_file(
        &service,
        &sample_dir("src/tests/samples/project_src_with_config_no_extra_paths"),
    );
    let mut command_line_options =
        CommandLineOptions::new(Some(cwd.get_file_path().to_string()), false);
    command_line_options.config_settings.auto_search_paths = Some(true);
    command_line_options.config_settings.extra_paths = Some(vec!["src/_vendored".to_string()]);
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);

    let expected_extra_paths: Vec<Uri> = vec![
        service.fs().real_case_path(cwd.combine_paths(&["src"])),
        service
            .fs()
            .real_case_path(cwd.combine_paths(&["src", "_vendored"])),
    ];

    assert_eq!(
        config_options.default_extra_paths,
        Some(expected_extra_paths)
    );
}

// tests/config.test.ts:451
#[test]
fn basic_pyproject_toml_parsing() {
    let cwd = sample_dir("src/tests/samples/project_with_pyproject_toml");
    let service = create_analyzer(None);
    let command_line_options = CommandLineOptions::new(Some(cwd), true);

    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);
    assert_eq!(
        PythonVersion::to_string(&config_options.default_python_version.unwrap()),
        PythonVersion::to_string(&PYTHON_VERSION3_9)
    );
    assert_eq!(
        config_options.diagnostic_rule_set.report_missing_imports,
        DiagnosticLevel::Error
    );
    assert_eq!(
        config_options.diagnostic_rule_set.report_unused_class,
        DiagnosticLevel::Warning
    );
}

// tests/config.test.ts:464
#[test]
#[ignore = "needs AnalyzerService.setFileOpened (open files are not ported) and an untitled: WebUri (not ported)"]
fn find_files_in_memory_only() {
    panic!("setFileOpened and WebUri are not ported");
}

// tests/config.test.ts:480
#[test]
#[ignore = "needs serialize/deserialize (backgroundThreadBase.ts) and the vfs TestFileSystem; neither is reachable here"]
fn verify_config_file_specs_after_cloning() {
    panic!("serialize/deserialize is not ported");
}

// tests/config.test.ts:495
#[test]
#[ignore = "needs serialize/deserialize (backgroundThreadBase.ts), which is not ported"]
fn verify_can_serialize_config_options() {
    panic!("serialize/deserialize is not ported");
}

// tests/config.test.ts:503
#[test]
#[ignore = "needs the vfs TestFileSystem and config file watchers; neither is reachable here"]
fn config_watcher_reloads_when_pyproject_toml_is_created() {
    panic!("file watchers and vfs.TestFileSystem are not ported");
}

// tests/config.test.ts:541
#[test]
fn extra_paths_on_undefined_execution_root_default_workspace() {
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let mut command_line_options = CommandLineOptions::new(None, false);
    command_line_options.config_settings.extra_paths = Some(vec!["/extraPaths".to_string()]);

    service.set_options(command_line_options.clone());
    let config_options = service.test_get_config_options(&command_line_options);

    let expected_extra_paths = [service_file(&service, "/extraPaths")];
    assert_eq!(
        config_options
            .default_extra_paths
            .as_ref()
            .map(|paths| paths.iter().map(|u| u.get_file_path()).collect::<Vec<_>>()),
        Some(
            expected_extra_paths
                .iter()
                .map(|u| u.get_file_path())
                .collect::<Vec<_>>()
        )
    );
}

// tests/config.test.ts:557
#[test]
fn extended_config_files() {
    let cwd = sample_dir("src/tests/samples/project_with_extended_config");
    let service = create_analyzer(None);
    let command_line_options = CommandLineOptions::new(Some(cwd), true);

    service.set_options(command_line_options.clone());

    let file_list = service.test_get_file_names_from_file_specs();
    assert_eq!(file_names(&file_list), ["sample.pyi", "test.py"]);

    let config_options = service.test_get_config_options(&command_line_options);
    assert!(config_options.diagnostic_rule_set.strict_list_inference);
}

// tests/config.test.ts:572
#[test]
fn typechecking_mode_is_standard_when_just_config_file_is_present() {
    let cwd = sample_dir("src/tests/samples/project_with_pyproject_toml");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd), true);
    command_line_options.config_settings.type_checking_mode = Some("off".to_string());

    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);
    assert_eq!(
        config_options
            .diagnostic_rule_set
            .report_possibly_unbound_variable,
        DiagnosticLevel::Error
    );
}

// tests/config.test.ts:584
#[test]
fn typechecking_mode_depends_upon_if_vscode_extension_or_not() {
    let cwd = sample_dir("src/tests/samples/package1");
    let mut service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd.clone()), true);

    service.set_options(command_line_options.clone());

    let mut config_options = service.test_get_config_options(&command_line_options);
    assert_eq!(
        config_options
            .diagnostic_rule_set
            .report_possibly_unbound_variable,
        DiagnosticLevel::None
    );

    service = create_analyzer(None);
    command_line_options = CommandLineOptions::new(Some(cwd.clone()), false);

    service.set_options(command_line_options.clone());

    config_options = service.test_get_config_options(&command_line_options);
    assert_eq!(
        config_options
            .diagnostic_rule_set
            .report_possibly_unbound_variable,
        DiagnosticLevel::Error
    );

    command_line_options = CommandLineOptions::new(Some(cwd), true);
    command_line_options.config_settings.type_checking_mode = Some("strict".to_string());
    service = create_analyzer(None);
    service.set_options(command_line_options.clone());

    config_options = service.test_get_config_options(&command_line_options);
    assert_eq!(
        config_options
            .diagnostic_rule_set
            .report_possibly_unbound_variable,
        DiagnosticLevel::Error
    );
}

// tests/config.test.ts:611
#[test]
fn include_file_paths_are_only_set_in_the_config_file_when_using_extension() {
    let cwd = sample_dir("src/tests/samples/project1");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd), true);
    command_line_options.config_settings.include_file_specs = vec!["test".to_string()];
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);
    assert_eq!(config_options.include.len(), 1);
    assert!(
        config_options.include[0]
            .reg_exp
            .as_str()
            .contains("/subfolder1)")
    );
}

// tests/config.test.ts:623
#[test]
fn include_file_paths_can_be_added_to_on_the_command_line_with_a_config() {
    let cwd = sample_dir("src/tests/samples/project1");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd), false);
    command_line_options.config_settings.include_file_specs = vec!["test".to_string()];
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);
    assert_eq!(config_options.include.len(), 2);
    assert!(
        config_options.include[1]
            .reg_exp
            .as_str()
            .contains("/test)")
    );
}

// tests/config.test.ts:635
#[test]
fn include_file_paths_can_be_added_to_by_an_extension_without_a_config() {
    let cwd = sample_dir("src/tests/samples/package1");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd), true);
    command_line_options.config_settings.include_file_specs = vec!["test".to_string()];
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);
    assert_eq!(config_options.include.len(), 1);
    assert!(
        config_options.include[0]
            .reg_exp
            .as_str()
            .contains("/test)")
    );
}

// tests/config.test.ts:647
#[test]
fn uri_scheme_file_specs_are_filtered_out_of_include_exclude_and_ignore() {
    // Regression test for https://github.com/microsoft/pylance-release/issues/6612.
    // In multi-root workspaces a folder can use a custom URI scheme (e.g. memfs:, zowe-uss:).
    // Those scheme strings must not be treated as file paths, otherwise they get combined with
    // the project root and corrupt the include/exclude/ignore specs.
    let cwd = sample_dir("src/tests/samples/project1");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd), false);
    command_line_options.config_settings.include_file_specs = vec![
        "memfs:/host/workspace".to_string(),
        "realInclude".to_string(),
    ];
    command_line_options.config_settings.exclude_file_specs = vec![
        "zowe-uss://host/exclude".to_string(),
        "realExclude".to_string(),
    ];
    command_line_options.config_settings.ignore_file_specs = vec![
        "vscode-vfs://host/ignore".to_string(),
        "realIgnore".to_string(),
    ];
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);

    let include_sources = reg_exp_sources(&config_options.include);
    let exclude_sources = reg_exp_sources(&config_options.exclude);
    let ignore_sources = reg_exp_sources(&config_options.ignore);

    // The URI scheme specs are dropped entirely (their scheme strings never appear).
    assert!(!include_sources.iter().any(|s| s.contains("memfs")));
    assert!(!exclude_sources.iter().any(|s| s.contains("zowe-uss")));
    assert!(!ignore_sources.iter().any(|s| s.contains("vscode-vfs")));

    // The real file specs alongside them are still applied.
    assert!(include_sources.iter().any(|s| s.contains("/realInclude)")));
    assert!(exclude_sources.iter().any(|s| s.contains("/realExclude)")));
    assert!(ignore_sources.iter().any(|s| s.contains("/realIgnore)")));
}

// tests/config.test.ts:677
#[test]
fn windows_drive_letter_file_specs_are_not_mistaken_for_uri_schemes() {
    // The URI scheme filter must require 2+ leading letters so single-letter Windows drive
    // prefixes like "C:" are preserved rather than filtered as if they were URI schemes.
    let cwd = sample_dir("src/tests/samples/project1");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd), false);
    command_line_options.config_settings.include_file_specs =
        vec!["C:/driveLetterInclude".to_string()];
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);
    let include_sources = reg_exp_sources(&config_options.include);
    assert!(
        include_sources
            .iter()
            .any(|s| s.contains("driveLetterInclude"))
    );
}

// tests/config.test.ts:691
#[test]
fn relative_paths_containing_a_colon_are_not_mistaken_for_uri_schemes() {
    // The URI scheme filter requires a "/" after the scheme delimiter so legitimate relative
    // paths that merely contain a colon (e.g. POSIX "foo:bar") are preserved, not dropped.
    let cwd = sample_dir("src/tests/samples/project1");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd), false);
    command_line_options.config_settings.include_file_specs = vec!["foo:bar".to_string()];
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);
    let include_sources = reg_exp_sources(&config_options.include);
    // The spec is not dropped by the URI scheme filter (if it were, include would be empty).
    // The colon is regex-escaped in the resulting pattern, so match on the surrounding path
    // components rather than the literal "foo:bar".
    assert!(
        include_sources
            .iter()
            .any(|s| s.contains("foo") && s.contains("bar"))
    );
}

// tests/config.test.ts:708
#[test]
fn uri_scheme_file_specs_are_filtered_out_of_include_file_specs_override() {
    // includeFileSpecsOverride strings flow into Uri.file(), so URI scheme strings must be
    // filtered there too, otherwise they corrupt the override include specs.
    let cwd = sample_dir("src/tests/samples/project1");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd.clone()), false);
    command_line_options
        .config_settings
        .include_file_specs_override = Some(vec![
        "memfs:/host/workspace".to_string(),
        combine_paths(&cwd, &["realOverride"]),
    ]);
    service.set_options(command_line_options.clone());

    let config_options = service.test_get_config_options(&command_line_options);
    let include_sources = reg_exp_sources(&config_options.include);

    // The URI scheme override is dropped, but the real override path is still applied.
    assert!(!include_sources.iter().any(|s| s.contains("memfs")));
    assert!(include_sources.iter().any(|s| s.contains("realOverride")));
}

// tests/config.test.ts:728
#[test]
fn command_line_options_can_override_config_but_only_when_not_using_extension() {
    let cwd = sample_dir("src/tests/samples/project_with_all_config");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd.clone()), false);
    service.set_options(command_line_options.clone());

    // First get the default.
    let default_options = service.test_get_config_options(&command_line_options);

    // Now set all of the different options and make sure the command line options override.
    command_line_options.config_settings.type_checking_mode = Some("strict".to_string());
    command_line_options.config_settings.venv_path = Some("test2".to_string());
    command_line_options.config_settings.typeshed_path = Some("test2".to_string());
    command_line_options.config_settings.stub_path = Some("test2".to_string());
    command_line_options
        .config_settings
        .use_library_code_for_types = Some(true);
    command_line_options.config_settings.include_file_specs = vec!["test2".to_string()];
    command_line_options.config_settings.exclude_file_specs = vec!["test2".to_string()];
    let mut diagnostic_severity_overrides = DiagnosticSeverityOverridesMap::default();
    diagnostic_severity_overrides.insert(
        "reportMissingImports".to_string(),
        DiagnosticOverride::Severity(DiagnosticLevel::Error),
    );
    command_line_options
        .config_settings
        .diagnostic_severity_overrides = Some(diagnostic_severity_overrides);
    command_line_options.config_settings.ignore_file_specs = vec!["test2".to_string()];

    service.set_options(command_line_options.clone());
    let overridden_options = service.test_get_config_options(&command_line_options);
    assert_ne!(
        file_spec_values(&default_options.include),
        file_spec_values(&overridden_options.include)
    );
    assert_ne!(
        file_spec_values(&default_options.exclude),
        file_spec_values(&overridden_options.exclude)
    );
    assert_ne!(
        file_spec_values(&default_options.ignore),
        file_spec_values(&overridden_options.ignore)
    );
    assert_ne!(
        default_options.diagnostic_rule_set,
        overridden_options.diagnostic_rule_set
    );
    assert_ne!(default_options.venv_path, overridden_options.venv_path);
    // Typeshed and stub path are an exception, it should just be reported as a dupe.
    assert_eq!(
        default_options.typeshed_path,
        overridden_options.typeshed_path
    );
    assert_eq!(default_options.stub_path, overridden_options.stub_path);

    // Do the same with an extension based config, but make sure we get the default back.
    let command_line_options2 = CommandLineOptions::new(Some(cwd), true);
    service.set_options(command_line_options2.clone());
    let overridden_options2 = service.test_get_config_options(&command_line_options2);
    assert_eq!(
        config_options_values(&default_options),
        config_options_values(&overridden_options2)
    );
}

// tests/config.test.ts:768
#[test]
fn config_venv_path_take_precedences_over_language_server_settings() {
    let cwd = sample_dir("src/tests/samples/project_with_all_config");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd), true);
    command_line_options.language_server_settings.venv_path =
        Some("test_from_language_server".to_string());
    service.set_options(command_line_options.clone());

    // Verify language server options don't override
    let options = service.test_get_config_options(&command_line_options);
    // `assert.equal(undefined, false)` fails, so `venvPath` must be set.
    assert_eq!(
        options
            .venv_path
            .map(|p| p.path_includes("from_language_server")),
        Some(false)
    );
}

// tests/config.test.ts:780
#[test]
fn command_line_venv_path_take_precedences_over_everything_else() {
    let cwd = sample_dir("src/tests/samples/project_with_all_config");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd), false);
    command_line_options.config_settings.venv_path = Some("test_from_command_line".to_string());
    command_line_options.language_server_settings.venv_path =
        Some("test_from_language_server".to_string());
    service.set_options(command_line_options.clone());

    // Verify command line overrides everything
    let options = service.test_get_config_options(&command_line_options);
    assert!(
        options
            .venv_path
            .is_some_and(|p| p.path_includes("test_from_command_line"))
    );
}

// tests/config.test.ts:793
#[test]
fn config_empty_venv_path_does_not_take_precedences_over_language_server_settings() {
    let cwd = sample_dir("src/tests/samples/project_src_with_config_extra_paths");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd), true);
    command_line_options.language_server_settings.venv_path =
        Some("test_from_language_server".to_string());
    service.set_options(command_line_options.clone());

    // Verify language server options don't override
    let options = service.test_get_config_options(&command_line_options);
    assert!(
        options
            .venv_path
            .is_some_and(|p| p.path_includes("from_language_server"))
    );
}

// tests/config.test.ts:805
#[test]
fn language_server_specific_settings_are_set_whether_or_not_there_is_a_pyproject_toml() {
    let cwd = sample_dir("src/tests/samples/project_with_all_config");
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd), false);
    command_line_options
        .language_server_settings
        .auto_import_completions = Some(true);
    command_line_options.language_server_settings.indexing = Some(true);
    command_line_options
        .language_server_settings
        .task_list_tokens = Some(vec![TaskListToken {
        priority: TaskListPriority::High,
        text: "test".to_string(),
    }]);
    command_line_options
        .language_server_settings
        .log_type_evaluation_time = true;
    command_line_options
        .language_server_settings
        .type_evaluation_time_threshold = 1.0;
    command_line_options
        .language_server_settings
        .enable_ambient_analysis = false;
    command_line_options
        .language_server_settings
        .disable_tagged_hints = Some(true);
    command_line_options
        .language_server_settings
        .watch_for_source_changes = Some(true);
    command_line_options
        .language_server_settings
        .watch_for_library_changes = Some(true);
    command_line_options
        .language_server_settings
        .watch_for_config_changes = Some(true);
    command_line_options
        .language_server_settings
        .check_only_open_files = Some(true);
    command_line_options
        .language_server_settings
        .disable_tagged_hints = Some(true);
    command_line_options.language_server_settings.python_path =
        Some("test_python_path".to_string());

    service.set_options(command_line_options.clone());
    let mut options = service.test_get_config_options(&command_line_options);
    assert!(options.auto_import_completions);
    assert!(options.indexing);
    assert_eq!(options.task_list_tokens.as_ref().map(Vec::len), Some(1));
    assert!(options.log_type_evaluation_time);
    assert_eq!(options.type_evaluation_time_threshold, 1.0);
    assert!(options.disable_tagged_hints);
    assert!(
        options
            .python_path
            .is_some_and(|p| p.path_includes("test_python_path"))
    );

    // Test with language server set to true to make sure they are still set.
    command_line_options.from_language_server = true;
    command_line_options.language_server_settings.venv_path = Some("test_venv_path".to_string());
    service.set_options(command_line_options.clone());
    options = service.test_get_config_options(&command_line_options);
    assert!(options.auto_import_completions);
    assert!(options.indexing);
    assert_eq!(options.task_list_tokens.as_ref().map(Vec::len), Some(1));
    assert!(options.log_type_evaluation_time);
    assert_eq!(options.type_evaluation_time_threshold, 1.0);
    assert!(options.disable_tagged_hints);
    assert!(
        options
            .python_path
            .is_some_and(|p| p.path_includes("test_python_path"))
    );

    // Verify language server options don't override the config setting. Only command line should
    // `assert.equal(undefined, false)` fails, so `venvPath` must be set.
    assert_eq!(
        options.venv_path.map(|p| p.path_includes("test_venv_path")),
        Some(false)
    );
}

// tests/config.test.ts:850
#[test]
fn default_python_version_no_config() {
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let mut command_line_options = CommandLineOptions::new(Some(cwd()), false);
    command_line_options.config_file_path = Some("src/tests/samples/package1".to_string());
    service.set_options(command_line_options.clone());

    let config = service.test_get_config_options(&command_line_options);
    assert_eq!(config.default_python_version, Some(PYTHON_VERSION3_13));
}

// tests/config.test.ts:862
#[test]
fn default_python_version_with_config() {
    let null_console = leak(NullConsole::default());
    let service = create_analyzer(Some(null_console));
    let mut command_line_options = CommandLineOptions::new(Some(cwd()), false);
    command_line_options.config_file_path = Some("src/tests/samples/project1".to_string());
    service.set_options(command_line_options.clone());

    let config = service.test_get_config_options(&command_line_options);
    assert_eq!(config.default_python_version, Some(PYTHON_VERSION3_13));
}

// tests/config.test.ts:874
#[test]
fn pyproject_toml_without_pyright_section_falls_back_to_ancestor_config() {
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd()), false);
    command_line_options.config_file_path =
        Some("src/tests/samples/project_with_empty_pyproject_toml/subproject".to_string());

    let config_options = service.test_get_config_options(&command_line_options);

    // Should fall back to the ancestor pyproject.toml which sets pythonVersion to 3.9.
    assert_eq!(
        PythonVersion::to_string(&config_options.default_python_version.unwrap()),
        PythonVersion::to_string(&PYTHON_VERSION3_9)
    );

    // configFileSource should point to the ancestor pyproject.toml, not the subproject one.
    assert!(config_options.config_file_source.is_some_and(|source| {
        source
            .to_uri_string()
            .ends_with("project_with_empty_pyproject_toml/pyproject.toml")
    }));
}

// tests/config.test.ts:891
#[test]
fn pyproject_toml_without_pyright_section_falls_back_through_multiple_ancestors() {
    let service = create_analyzer(None);
    let mut command_line_options = CommandLineOptions::new(Some(cwd()), false);
    command_line_options.config_file_path = Some(
        "src/tests/samples/project_with_nested_empty_pyproject_toml/middle/subproject".to_string(),
    );

    let config_options = service.test_get_config_options(&command_line_options);

    // Should skip the empty pyproject.tomls and fall back to the root's [tool.pyright].
    assert_eq!(
        PythonVersion::to_string(&config_options.default_python_version.unwrap()),
        PythonVersion::to_string(&PYTHON_VERSION3_9)
    );
    assert!(config_options.config_file_source.is_some_and(|source| {
        source
            .to_uri_string()
            .ends_with("project_with_nested_empty_pyproject_toml/pyproject.toml")
    }));
}

// tests/config.test.ts:909
#[test]
fn diagnostic_rule_overrides_are_preserved_when_positional_args_override_include() {
    let cwd = sample_dir("src/tests/samples/project_with_diag_overrides");
    let service = create_analyzer(None);
    let command_line_options = CommandLineOptions::new(Some(cwd.clone()), false);
    service.set_options(command_line_options.clone());

    // Get config without include override - should have reportPrivateImportUsage: 'none'
    // because the config sets it to false.
    let config_without_override = service.test_get_config_options(&command_line_options);
    assert_eq!(
        config_without_override
            .diagnostic_rule_set
            .report_private_import_usage,
        DiagnosticLevel::None
    );

    // The basic default would be 'error', verify our config overrides it.
    let basic_defaults = get_basic_diagnostic_rule_set();
    assert_eq!(
        basic_defaults.report_private_import_usage,
        DiagnosticLevel::Error
    );

    // Now simulate positional args overriding include (like `pyright --project config.json subdir`).
    let mut command_line_options_with_override = CommandLineOptions::new(Some(cwd.clone()), false);
    command_line_options_with_override
        .config_settings
        .include_file_specs_override = Some(vec![combine_paths(&cwd, &["subdir"])]);
    service.set_options(command_line_options_with_override.clone());

    let config_with_override = service.test_get_config_options(&command_line_options_with_override);

    // The diagnostic rule overrides from the config file should still be applied
    // even when positional args replace the include paths.
    assert_eq!(
        config_with_override
            .diagnostic_rule_set
            .report_private_import_usage,
        DiagnosticLevel::None
    );

    // The execution environment for a file in the override path should also
    // have the config's diagnostic rule overrides.
    let file_uri = service_file(&service, &combine_paths(&cwd, &["subdir", "sample.py"]));
    let exec_env = config_with_override.find_exec_environment(file_uri);
    assert_eq!(
        exec_env.diagnostic_rule_set.report_private_import_usage,
        DiagnosticLevel::None
    );
}
