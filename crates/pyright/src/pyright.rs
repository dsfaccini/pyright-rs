//! Port of `packages/pyright-internal/src/pyright.ts`.
//!
//! Ported: argument parsing, the single-threaded check and its JSON and text
//! reports. Out of scope (AGENTS.md): `--watch`, `--threads` with more than
//! one thread, `--verifytypes` and `--createstub`; each stops with a message.
//! `--stats` prints the summary line only.

use std::cell::Cell;
use std::io::Read;
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use jsrt::command_line_args::{ArgValue, OptionDefinition, OptionType, command_line_args};
use jsrt::number::{number_to_string, parse_int};
use jsrt::{JsStr, P};

use crate::analyzer::analysis::AnalysisResults;
use crate::analyzer::service::{AnalyzerService, AnalyzerServiceOptions};
use crate::common::command_line_options::CommandLineOptions;
use crate::common::console::{ConsoleInterface, LogLevel, StandardConsole, StderrConsole, node_stderr, node_stdout};
use crate::common::diagnostic::{Diagnostic, DiagnosticCategory, compare_diagnostics};
use crate::common::diagnostic_sink::FileDiagnostics;
use crate::common::file_system::FileSystem;
use crate::common::full_access_host::FullAccessHost;
use crate::common::host::Host;
use crate::common::path_utils::{combine_paths, normalize_path};
use crate::common::python_version::PythonVersion;
use crate::common::real_file_system::{RealTempFile, create_from_real_file_system};
use crate::common::service_provider::ServiceProvider;
use crate::common::text_range::{Range, is_empty_range};
use crate::common::uri::Uri;
use crate::common::uri::uri::process_cwd;
use crate::common::uri::uri_utils::{get_file_spec, try_stat};
use crate::oracle::dumper::{init as oracle_init, oracle_dump_final};
use crate::pyright_file_system::PyrightFileSystem;

// pyright.ts:56
const TOOL_NAME: &str = "pyright";

/// `require('../package.json').version` of pyright-internal at the pinned release.
const VERSION: &str = "1.1.414";

// pyright.ts:58
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SeverityLevel {
    Error,
    Warning,
    Information,
}

impl SeverityLevel {
    fn as_str(self) -> &'static str {
        match self {
            SeverityLevel::Error => "error",
            SeverityLevel::Warning => "warning",
            SeverityLevel::Information => "information",
        }
    }
}

// pyright.ts:61
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    NoErrors = 0,
    ErrorsReported = 1,
    FatalError = 2,
    ConfigFileParseError = 3,
    ParameterError = 4,
}

// pyright.ts:121
struct PyrightJsonDiagnostic {
    file: String,
    severity: SeverityLevel,
    message: String,
    range: Option<Range>,
    rule: Option<String>,
}

// pyright.ts:139
struct DiagnosticResult {
    error_count: u32,
    warning_count: u32,
}

/// The parsed arguments, as `commandLineArgs` returns them.
struct Args {
    entries: Vec<(&'static str, ArgValue)>,
}

impl Args {
    fn get(&self, name: &str) -> Option<&ArgValue> {
        self.entries.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    /// `args[name] !== undefined`.
    fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// `!!args[name]`.
    fn truthy(&self, name: &str) -> bool {
        match self.get(name) {
            None | Some(ArgValue::Null) => false,
            Some(ArgValue::Bool(b)) => *b,
            Some(ArgValue::String(s)) => !s.is_empty(),
            Some(ArgValue::Number(n)) => *n != 0.0 && !n.is_nan(),
            Some(ArgValue::Array(_)) => true,
        }
    }

    /// `args[name]` when it is a non-empty string (the `if (args.x)` tests).
    fn string(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(ArgValue::String(s)) if !s.is_empty() => Some(s),
            _ => None,
        }
    }
}

// pyright.ts:1387
fn parse_threads_arg_value(input: &str) -> ArgValue {
    if input == "auto" {
        return ArgValue::Null;
    }

    let value = parse_int(input, 10);
    if value.is_nan() || value < 1.0 {
        return ArgValue::Null;
    }

    ArgValue::Number(value)
}

// pyright.ts:158
const OPTION_DEFINITIONS: [OptionDefinition; 24] = [
    OptionDefinition::new("createstub", OptionType::String),
    OptionDefinition::new("dependencies", OptionType::Boolean),
    OptionDefinition::new("files", OptionType::String).multiple_default(),
    OptionDefinition::new("help", OptionType::Boolean).alias('h'),
    OptionDefinition::new("ignoreexternal", OptionType::Boolean),
    OptionDefinition::new("lib", OptionType::Boolean),
    OptionDefinition::new("level", OptionType::String),
    OptionDefinition::new("outputjson", OptionType::Boolean),
    OptionDefinition::new("project", OptionType::String).alias('p'),
    OptionDefinition::new("pythonpath", OptionType::String),
    OptionDefinition::new("pythonplatform", OptionType::String),
    OptionDefinition::new("pythonversion", OptionType::String),
    OptionDefinition::new("skipunannotated", OptionType::Boolean),
    OptionDefinition::new("stats", OptionType::Boolean),
    OptionDefinition::new("threads", OptionType::Custom(parse_threads_arg_value)),
    OptionDefinition::new("typeshed-path", OptionType::String),
    OptionDefinition::new("typeshedpath", OptionType::String).alias('t'),
    OptionDefinition::new("venv-path", OptionType::String),
    OptionDefinition::new("venvpath", OptionType::String).alias('v'),
    OptionDefinition::new("verifytypes", OptionType::String),
    OptionDefinition::new("verbose", OptionType::Boolean),
    OptionDefinition::new("version", OptionType::Boolean),
    OptionDefinition::new("warnings", OptionType::Boolean),
    OptionDefinition::new("watch", OptionType::Boolean).alias('w'),
];

// pyright.ts:157
fn process_args(argv: &[String]) -> ExitStatus {
    let start_time = Instant::now();
    let args = match command_line_args(&OPTION_DEFINITIONS, argv) {
        Ok(entries) => Args { entries },
        Err(e) => {
            if let Some(option_name) = e.option_name() {
                node_stderr(&format!("Unexpected option {option_name}.\n{TOOL_NAME} --help for usage"));
                return ExitStatus::ParameterError;
            }

            node_stderr(&format!("Unexpected error\n{TOOL_NAME} --help for usage"));
            return ExitStatus::ParameterError;
        }
    };

    if args.has("help") {
        print_usage();
        return ExitStatus::NoErrors;
    }

    if args.has("version") {
        node_stdout(&format!("{TOOL_NAME} {VERSION}"));
        return ExitStatus::NoErrors;
    }

    for (arg, value) in &args.entries {
        if *value == ArgValue::Null && *arg != "threads" {
            node_stderr(&format!("'{arg}' option requires a value"));
            return ExitStatus::ParameterError;
        }
    }

    if args.has("outputjson") {
        for arg in ["stats", "verbose", "createstub", "dependencies"] {
            if args.has(arg) {
                node_stderr(&format!("'outputjson' option cannot be used with '{arg}' option"));
                return ExitStatus::ParameterError;
            }
        }
    }

    if args.has("verifytypes") {
        for arg in ["watch", "stats", "createstub", "dependencies", "skipunannotated", "threads"] {
            if args.has(arg) {
                node_stderr(&format!("'verifytypes' option cannot be used with '{arg}' option"));
                return ExitStatus::ParameterError;
            }
        }
    }

    if args.truthy("createstub") {
        for arg in ["watch", "stats", "verifytypes", "dependencies", "skipunannotated", "threads"] {
            if args.has(arg) {
                node_stderr(&format!("'createstub' option cannot be used with '{arg}' option"));
                return ExitStatus::ParameterError;
            }
        }
    }

    if args.truthy("threads") {
        for arg in ["watch", "stats", "dependencies"] {
            if args.has(arg) {
                node_stderr(&format!("'threads' option cannot be used with '{arg}' option"));
                return ExitStatus::ParameterError;
            }
        }
    }

    let cwd = process_cwd();
    let mut options = CommandLineOptions::new(Some(cwd.clone()), false);
    let temp_file: &'static RealTempFile = Box::leak(Box::new(RealTempFile::new()));

    // Assume any relative paths are relative to the working directory.
    if let Some(ArgValue::Array(files)) = args.get("files") {
        let mut file_spec_list: Vec<String> = files
            .iter()
            .filter_map(|f| match f {
                ArgValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect();

        // Has the caller indicated that the file list will be supplied by stdin?
        if file_spec_list.len() == 1 && file_spec_list[0] == "-" {
            let mut std_text = String::new();
            if std::io::stdin().read_to_string(&mut std_text).is_err() {
                node_stderr("Invalid file list specified by stdin input");
                return ExitStatus::ParameterError;
            }
            let replaced: String = std_text.chars().map(|c| if c == '\r' || c == '\n' { ' ' } else { c }).collect();
            file_spec_list = replaced
                .as_str()
                .js_trim()
                .split(' ')
                .map(|s| s.js_trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }

        let include_file_specs_override: Vec<String> =
            file_spec_list.iter().map(|f| combine_paths(&cwd, &[f])).collect();

        // Verify the specified file specs to make sure their wildcard roots exist.
        let temp_file_system = create_from_real_file_system(temp_file, Box::leak(Box::new(StandardConsole::new(LogLevel::Error))));
        for file_desc in &include_file_specs_override {
            let include_spec = get_file_spec(Uri::file(&cwd, temp_file, false), file_desc);
            if try_stat(&temp_file_system, include_spec.wildcard_root).is_none() {
                node_stderr(&format!(
                    "File or directory \"{}\" does not exist",
                    include_spec.wildcard_root.to_uri_string()
                ));
                return ExitStatus::ParameterError;
            }
        }

        options.config_settings.include_file_specs_override = Some(include_file_specs_override);
    }

    if let Some(project) = args.string("project") {
        options.config_file_path = Some(combine_paths(&cwd, &[&normalize_path(project)]));
    }

    if let Some(python_platform) = args.string("pythonplatform") {
        if ["Darwin", "Linux", "Windows", "iOS", "Android"].contains(&python_platform) {
            options.config_settings.python_platform = Some(python_platform.to_string());
        } else {
            node_stderr(&format!(
                "'{python_platform}' is not a supported Python platform; specify Darwin, Linux, Windows, iOS, or Android."
            ));
            return ExitStatus::ParameterError;
        }
    }

    if let Some(python_version) = args.string("pythonversion") {
        match PythonVersion::from_string(python_version) {
            Some(version) => options.config_settings.python_version = Some(version),
            None => {
                node_stderr(&format!(
                    "'{python_version}' is not a supported Python version; specify 3.3, 3.4, etc."
                ));
                return ExitStatus::ParameterError;
            }
        }
    }

    if let Some(ArgValue::String(python_path)) = args.get("pythonpath") {
        for arg in ["venv-path", "venvpath"] {
            if args.has(arg) {
                node_stderr(&format!("'pythonpath' option cannot be used with '{arg}' option"));
                return ExitStatus::ParameterError;
            }
        }

        options.config_settings.python_path = Some(combine_paths(&cwd, &[&normalize_path(python_path)]));
    }

    if let Some(venv_path) = args.string("venv-path") {
        node_stderr("'venv-path' option is deprecated; use 'venvpath' instead");
        options.config_settings.venv_path = Some(combine_paths(&cwd, &[&normalize_path(venv_path)]));
    }

    if let Some(venv_path) = args.string("venvpath") {
        options.config_settings.venv_path = Some(combine_paths(&cwd, &[&normalize_path(venv_path)]));
    }

    if let Some(typeshed_path) = args.string("typeshed-path") {
        node_stderr("'typeshed-path' option is deprecated; use 'typeshedpath' instead");
        options.config_settings.typeshed_path = Some(combine_paths(&cwd, &[&normalize_path(typeshed_path)]));
    }

    if let Some(typeshed_path) = args.string("typeshedpath") {
        options.config_settings.typeshed_path = Some(combine_paths(&cwd, &[&normalize_path(typeshed_path)]));
    }

    if args.truthy("skipunannotated") {
        options.config_settings.analyze_unannotated_functions = Some(false);
    }

    if args.truthy("verbose") {
        options.config_settings.verbose_output = Some(true);
    }

    // Always enable autoSearchPaths when using the command line.
    options.config_settings.auto_search_paths = Some(true);

    if args.truthy("lib") {
        node_stderr("The --lib option is deprecated. Pyright now defaults to using library code to infer types.");
    }

    let mut min_severity_level = SeverityLevel::Information;
    if let Some(level) = args.string("level") {
        let level_value = level.js_to_lower_case();
        if level_value == "error" {
            min_severity_level = SeverityLevel::Error;
        } else if level_value == "warning" {
            min_severity_level = SeverityLevel::Warning;
        } else {
            node_stderr(&format!("'{level}' is not a valid value for --level; specify error or warning."));
            return ExitStatus::ParameterError;
        }
    }

    options.language_server_settings.check_only_open_files = Some(false);

    if args.truthy("stats") && args.truthy("verbose") {
        options.language_server_settings.log_type_evaluation_time = true;
    }

    let log_level = if args.truthy("stats") || args.truthy("verbose") { LogLevel::Info } else { LogLevel::Error };

    // If using outputjson, redirect all console output to stderr so it doesn't mess
    // up the JSON output, which goes to stdout.
    let output: &'static dyn ConsoleInterface = if args.truthy("outputjson") {
        Box::leak(Box::new(StderrConsole::new(log_level)))
    } else {
        Box::leak(Box::new(StandardConsole::new(log_level)))
    };
    let real_file_system: &'static dyn FileSystem =
        Box::leak(Box::new(create_from_real_file_system(temp_file, output)));
    let file_system: &'static dyn FileSystem = Box::leak(Box::new(PyrightFileSystem::new(real_file_system)));

    let service_provider = ServiceProvider::new(file_system, output, temp_file);

    if args.has("verifytypes") {
        node_stderr("pyright-rs: --verifytypes is not supported");
        return ExitStatus::FatalError;
    } else if args.has("ignoreexternal") {
        node_stderr("'--ignoreexternal' is valid only when used with '--verifytypes'");
        return ExitStatus::ParameterError;
    }

    let watch = args.has("watch");
    if watch {
        node_stderr("pyright-rs: --watch is not supported");
        return ExitStatus::FatalError;
    }
    if args.truthy("createstub") {
        node_stderr("pyright-rs: --createstub is not supported");
        return ExitStatus::FatalError;
    }
    options.language_server_settings.watch_for_source_changes = Some(watch);
    options.language_server_settings.watch_for_config_changes = Some(watch);

    let service = AnalyzerService::new(
        "<default>",
        service_provider,
        AnalyzerServiceOptions {
            console: output,
            host_factory: Box::new(move || -> &'static dyn Host {
                Box::leak(Box::new(FullAccessHost::new(file_system, temp_file)))
            }),
            max_analysis_time: None,
            skip_scanning_user_files: false,
        },
    );

    // `--threads` with more than one thread runs worker processes, which the
    // port does not do; every run is single-threaded.
    run_single_threaded(&args, options, &service, min_severity_level, output, start_time)
}

// pyright.ts:458
fn run_single_threaded(
    args: &Args,
    options: CommandLineOptions,
    service: &AnalyzerService,
    min_severity_level: SeverityLevel,
    output: &'static dyn ConsoleInterface,
    start_time: Instant,
) -> ExitStatus {
    let treat_warnings_as_errors = args.truthy("warnings");
    let output_json = args.truthy("outputjson");

    // A deferred: the first resolve wins.
    let exit_status: Rc<Cell<Option<ExitStatus>>> = Rc::new(Cell::new(None));
    let resolve = {
        let exit_status = exit_status.clone();
        move |status: ExitStatus| {
            if exit_status.get().is_none() {
                exit_status.set(Some(status));
            }
        }
    };

    let program = service.program();
    service.set_completion_callback(Some(Box::new(move |results: AnalysisResults| {
        if results.fatal_error_occurred {
            resolve(ExitStatus::FatalError);
            return;
        }

        if results.config_parse_error_occurred {
            resolve(ExitStatus::ConfigFileParseError);
            return;
        }

        oracle_dump_final(&program);

        let mut error_count = 0;

        // Sort all file diagnostics by the file URI so
        // we have a deterministic ordering.
        let mut file_diagnostics = results.diagnostics;
        jsrt::sort::sort_by(&mut file_diagnostics, |a, b| {
            if a.file_uri.to_uri_string().as_str().js_cmp(b.file_uri.to_uri_string().as_str()).is_lt() { -1 } else { 1 }
        });

        if output_json {
            let report =
                report_diagnostics_as_json(&mut file_diagnostics, min_severity_level, results.files_in_program, results.elapsed_time);
            error_count += report.error_count;
            if treat_warnings_as_errors {
                error_count += report.warning_count;
            }
        } else {
            output.info(&format!("{TOOL_NAME} {VERSION}"));
            let report = report_diagnostics_as_text(&mut file_diagnostics, min_severity_level);
            error_count += report.error_count;
            if treat_warnings_as_errors {
                error_count += report.warning_count;
            }
        }

        if !output_json {
            // Print the total time.
            output.info(&format!("Completed in {}sec", duration_in_seconds(start_time)));
        }

        resolve(if error_count > 0 { ExitStatus::ErrorsReported } else { ExitStatus::NoErrors });
    })));

    // This will trigger the analyzer.
    service.set_options(options);
    service.enumerate_source_files(0.0);

    // Node's event loop: fire the analysis timer.
    service.run_timers();

    exit_status.get().unwrap_or(ExitStatus::NoErrors)
}

/// `Duration.getDurationInSeconds()`: whole milliseconds, as `Date.now()` counts them, over 1000.
fn duration_in_seconds(start: Instant) -> String {
    number_to_string(start.elapsed().as_millis() as f64 / 1000.0)
}

// pyright.ts:1170
fn print_usage() {
    node_stdout(&format!(
        "Usage: {TOOL_NAME} [options] files...\n\
         \x20 Options:\n\
         \x20 --createstub <IMPORT>              Create type stub file(s) for import\n\
         \x20 --dependencies                     Emit import dependency information\n\
         \x20 -h,--help                          Show this help message\n\
         \x20 --ignoreexternal                   Ignore external imports for --verifytypes\n\
         \x20 --level <LEVEL>                    Minimum diagnostic level (error or warning)\n\
         \x20 --outputjson                       Output results in JSON format\n\
         \x20 -p,--project <FILE OR DIRECTORY>   Use the configuration file at this location\n\
         \x20 --pythonplatform <PLATFORM>        Analyze for a specific platform (Darwin, Linux, Windows, iOS, Android)\n\
         \x20 --pythonpath <FILE>                Path to the Python interpreter\n\
         \x20 --pythonversion <VERSION>          Analyze for a specific version (3.3, 3.4, etc.)\n\
         \x20 --skipunannotated                  Skip analysis of functions with no type annotations\n\
         \x20 --stats                            Print detailed performance stats\n\
         \x20 -t,--typeshedpath <DIRECTORY>      Use typeshed type stubs at this location\n\
         \x20 --threads <optional COUNT>         Use separate threads to parallelize type checking \n\
         \x20 -v,--venvpath <DIRECTORY>          Directory that contains virtual environments\n\
         \x20 --verbose                          Emit verbose diagnostics\n\
         \x20 --verifytypes <PACKAGE>            Verify type completeness of a py.typed package\n\
         \x20 --version                          Print Pyright version and exit\n\
         \x20 --warnings                         Use exit code of 1 if warnings are reported\n\
         \x20 -w,--watch                         Continue to run and watch for changes\n\
         \x20 -                                  Read files from stdin\n"
    ));
}

// pyright.ts:1210
fn report_diagnostics_as_json(
    file_diagnostics: &mut [FileDiagnostics],
    min_severity_level: SeverityLevel,
    files_in_program: usize,
    time_in_sec: f64,
) -> DiagnosticResult {
    let time = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let mut general_diagnostics: Vec<PyrightJsonDiagnostic> = Vec::new();
    let mut error_count = 0;
    let mut warning_count = 0;
    let mut information_count = 0;

    for file_diag in file_diagnostics.iter_mut() {
        jsrt::sort::sort_by(&mut file_diag.diagnostics, |a, b| compare_diagnostics(a, b));
        for diag in &file_diag.diagnostics {
            if matches!(
                diag.category,
                DiagnosticCategory::Error | DiagnosticCategory::Warning | DiagnosticCategory::Information
            ) {
                let json_diag = convert_diagnostic_to_json(file_diag.file_uri.get_file_path(), diag);
                // pyright.ts:932 accumulateReportDiagnosticStats
                match json_diag.severity {
                    SeverityLevel::Error => error_count += 1,
                    SeverityLevel::Warning => warning_count += 1,
                    SeverityLevel::Information => information_count += 1,
                }
                if is_diagnostic_included(json_diag.severity, min_severity_level) {
                    general_diagnostics.push(json_diag);
                }
            }
        }
    }

    // `JSON.stringify(report, undefined, 4)`.
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("    \"version\": {},\n", VERSION.js_json_stringify()));
    out.push_str(&format!("    \"time\": {},\n", time.to_string().js_json_stringify()));
    if general_diagnostics.is_empty() {
        out.push_str("    \"generalDiagnostics\": [],\n");
    } else {
        out.push_str("    \"generalDiagnostics\": [\n");
        for (i, diag) in general_diagnostics.iter().enumerate() {
            out.push_str("        {\n");
            let mut fields = vec![
                format!("\"file\": {}", diag.file.js_json_stringify()),
                format!("\"severity\": {}", diag.severity.as_str().js_json_stringify()),
                format!("\"message\": {}", diag.message.js_json_stringify()),
            ];
            if let Some(range) = &diag.range {
                fields.push(format!(
                    "\"range\": {{\n                \"start\": {{\n                    \"line\": {},\n                    \"character\": {}\n                }},\n                \"end\": {{\n                    \"line\": {},\n                    \"character\": {}\n                }}\n            }}",
                    range.start.line, range.start.character, range.end.line, range.end.character
                ));
            }
            if let Some(rule) = &diag.rule {
                fields.push(format!("\"rule\": {}", rule.js_json_stringify()));
            }
            out.push_str(&fields.iter().map(|f| format!("            {f}")).collect::<Vec<_>>().join(",\n"));
            out.push_str("\n        }");
            out.push_str(if i + 1 < general_diagnostics.len() { ",\n" } else { "\n" });
        }
        out.push_str("    ],\n");
    }
    out.push_str("    \"summary\": {\n");
    out.push_str(&format!("        \"filesAnalyzed\": {files_in_program},\n"));
    out.push_str(&format!("        \"errorCount\": {error_count},\n"));
    out.push_str(&format!("        \"warningCount\": {warning_count},\n"));
    out.push_str(&format!("        \"informationCount\": {information_count},\n"));
    out.push_str(&format!("        \"timeInSec\": {}\n", number_to_string(time_in_sec)));
    out.push_str("    }\n}");
    node_stdout(&out);

    // Output a blank line to help tools that are attempting to parse the
    // JSON output when used in watch mode.
    node_stdout("");

    DiagnosticResult { error_count, warning_count }
}

// pyright.ts:1260
fn is_diagnostic_included(diag_severity: SeverityLevel, min_severity_level: SeverityLevel) -> bool {
    // Errors are always included.
    if diag_severity == SeverityLevel::Error {
        return true;
    }

    // Warnings are included only if the min severity level is below error.
    if diag_severity == SeverityLevel::Warning {
        return min_severity_level != SeverityLevel::Error;
    }

    // Informations are included only if the min severity level is 'information'.
    min_severity_level == SeverityLevel::Information
}

// pyright.ts:1275
fn convert_diagnostic_category_to_severity(category: DiagnosticCategory) -> SeverityLevel {
    match category {
        DiagnosticCategory::Error => SeverityLevel::Error,
        DiagnosticCategory::Warning => SeverityLevel::Warning,
        DiagnosticCategory::Information => SeverityLevel::Information,
        _ => crate::common::debug::fail(Some("Unexpected diagnostic category")),
    }
}

// pyright.ts:1291
fn convert_diagnostic_to_json(file_path: &str, diag: &Diagnostic) -> PyrightJsonDiagnostic {
    PyrightJsonDiagnostic {
        file: file_path.to_string(),
        severity: convert_diagnostic_category_to_severity(diag.category),
        message: diag.message.clone(),
        range: if is_empty_range(&diag.range) { None } else { Some(diag.range) },
        rule: diag.get_rule(),
    }
}

// pyright.ts:1301
fn report_diagnostics_as_text(file_diagnostics: &mut [FileDiagnostics], min_severity_level: SeverityLevel) -> DiagnosticResult {
    let mut error_count = 0;
    let mut warning_count = 0;
    let mut information_count = 0;

    for file_diagnostics in file_diagnostics.iter() {
        // Don't report unused code or deprecated diagnostics.
        let mut file_errors_and_warnings: Vec<P<Diagnostic>> = file_diagnostics
            .diagnostics
            .iter()
            .copied()
            .filter(|diag| {
                !matches!(
                    diag.category,
                    DiagnosticCategory::UnusedCode | DiagnosticCategory::UnreachableCode | DiagnosticCategory::Deprecated
                ) && is_diagnostic_included(convert_diagnostic_category_to_severity(diag.category), min_severity_level)
            })
            .collect();
        jsrt::sort::sort_by(&mut file_errors_and_warnings, |a, b| compare_diagnostics(a, b));

        if !file_errors_and_warnings.is_empty() {
            node_stdout(file_diagnostics.file_uri.to_user_visible_string());
            for diag in &file_errors_and_warnings {
                let json_diag = convert_diagnostic_to_json(file_diagnostics.file_uri.get_file_path(), diag);
                log_diagnostic_to_console(&json_diag, "  ");

                match diag.category {
                    DiagnosticCategory::Error => error_count += 1,
                    DiagnosticCategory::Warning => warning_count += 1,
                    DiagnosticCategory::Information => information_count += 1,
                    _ => {}
                }
            }
        }
    }

    node_stdout(&format!(
        "{error_count} {}, {warning_count} {}, {information_count} {}",
        if error_count == 1 { "error" } else { "errors" },
        if warning_count == 1 { "warning" } else { "warnings" },
        if information_count == 1 { "information" } else { "informations" }
    ));

    DiagnosticResult { error_count, warning_count }
}

// pyright.ts:1352
/// Chalk adds no color when stdout is not a terminal; this prints plain text.
fn log_diagnostic_to_console(diag: &PyrightJsonDiagnostic, prefix: &str) {
    let mut message = prefix.to_string();
    if !diag.file.is_empty() {
        message.push_str(&format!("{}:", diag.file));
    }
    match &diag.range {
        Some(range) if !is_empty_range(range) => {
            message.push_str(&format!("{}:{} - ", range.start.line + 1, range.start.character + 1));
        }
        _ => message.push(' '),
    }

    let mut lines = diag.message.split('\n');
    let first_line = lines.next().unwrap_or("");
    let remaining_lines: Vec<&str> = lines.collect();

    message.push_str(diag.severity.as_str());
    message.push_str(&format!(": {first_line}"));
    if !remaining_lines.is_empty() {
        message.push_str(&format!("\n{prefix}{}", remaining_lines.join(&format!("\n{prefix}"))));
    }

    if let Some(rule) = &diag.rule {
        message.push_str(&format!(" ({rule})"));
    }

    node_stdout(&message);
}

// pyright.ts:1404
/// Runs the CLI on a thread with a 1 GiB stack, since the analyzer recurses
/// as deeply as pyright does on V8's stack.
pub fn main() -> i32 {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let handle = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(move || {
            oracle_init();
            process_args(&argv) as i32
        })
        .expect("spawn the analysis thread");
    handle.join().unwrap_or(ExitStatus::FatalError as i32)
}
