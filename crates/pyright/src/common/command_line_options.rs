//! Port of `packages/pyright-internal/src/common/commandLineOptions.ts`.

use jsrt::JsMap;

use crate::common::config_options::DiagnosticOverride;
use crate::common::diagnostic::TaskListToken;
use crate::common::python_version::PythonVersion;

// common/commandLineOptions.ts:32
pub type DiagnosticSeverityOverridesMap = JsMap<String, DiagnosticOverride>;

// common/commandLineOptions.ts:33
pub type DiagnosticBooleanOverridesMap = JsMap<String, DiagnosticOverride>;

// common/commandLineOptions.ts:37
#[derive(Clone, Debug, Default)]
pub struct CommandLineConfigOptions {
    pub include_file_specs: Vec<String>,
    pub include_file_specs_override: Option<Vec<String>>,
    pub exclude_file_specs: Vec<String>,
    pub ignore_file_specs: Vec<String>,
    pub venv_path: Option<String>,
    pub python_path: Option<String>,
    pub python_environment_name: Option<String>,
    pub python_platform: Option<String>,
    pub python_version: Option<PythonVersion>,
    pub typeshed_path: Option<String>,
    pub stub_path: Option<String>,
    pub use_library_code_for_types: Option<bool>,
    pub auto_search_paths: Option<bool>,
    pub use_default_excludes: Option<bool>,
    pub extra_paths: Option<Vec<String>>,
    pub type_checking_mode: Option<String>,
    pub diagnostic_severity_overrides: Option<DiagnosticSeverityOverridesMap>,
    pub diagnostic_boolean_overrides: Option<DiagnosticBooleanOverridesMap>,
    pub analyze_unannotated_functions: Option<bool>,
    pub verbose_output: Option<bool>,
}

// common/commandLineOptions.ts:115
#[derive(Clone, Debug)]
pub struct CommandLineLanguageServerOptions {
    pub watch_for_source_changes: Option<bool>,
    pub watch_for_library_changes: Option<bool>,
    pub watch_for_config_changes: Option<bool>,
    pub check_only_open_files: Option<bool>,
    pub auto_import_completions: Option<bool>,
    pub indexing: Option<bool>,
    pub task_list_tokens: Option<Vec<TaskListToken>>,
    pub log_type_evaluation_time: bool,
    pub type_evaluation_time_threshold: f64,
    pub enable_ambient_analysis: bool,
    pub disable_tagged_hints: Option<bool>,
    pub python_path: Option<String>,
    pub venv_path: Option<String>,
}

impl Default for CommandLineLanguageServerOptions {
    fn default() -> Self {
        CommandLineLanguageServerOptions {
            watch_for_source_changes: None,
            watch_for_library_changes: None,
            watch_for_config_changes: None,
            check_only_open_files: None,
            auto_import_completions: None,
            indexing: None,
            task_list_tokens: None,
            log_type_evaluation_time: false,
            type_evaluation_time_threshold: 50.0,
            enable_ambient_analysis: true,
            disable_tagged_hints: None,
            python_path: None,
            venv_path: None,
        }
    }
}

// common/commandLineOptions.ts:161
#[derive(Clone, Debug)]
pub struct CommandLineOptions {
    pub config_settings: CommandLineConfigOptions,
    pub language_server_settings: CommandLineLanguageServerOptions,
    pub config_file_path: Option<String>,
    /// `string | Uri | undefined`; the CLI passes `process.cwd()`.
    pub execution_root: Option<String>,
    pub from_language_server: bool,
}

impl CommandLineOptions {
    // common/commandLineOptions.ts:180
    pub fn new(execution_root: Option<String>, from_language_server: bool) -> CommandLineOptions {
        CommandLineOptions {
            config_settings: CommandLineConfigOptions::default(),
            language_server_settings: CommandLineLanguageServerOptions::default(),
            config_file_path: None,
            execution_root,
            from_language_server,
        }
    }
}
