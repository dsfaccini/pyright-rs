//! Port of `packages/pyright-internal/src/common/fullAccessHost.ts`.

use std::process::{Command, Stdio};

use jsrt::JsStr;
use serde_json::Value;

use crate::analyzer::import_logger::ImportLogger;
use crate::analyzer::python_path_utils::PythonPathResult;
use crate::common::case_sensitivity_detector::CaseSensitivityDetector;
use crate::common::config_options::PythonPlatform;
use crate::common::file_system::FileSystem;
use crate::common::host::{Host, HostKind};
use crate::common::path_utils::normalize_path;
use crate::common::python_version::{PythonReleaseLevel, PythonVersion};
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::{get_usable_uri_path, is_usable_directory};

// common/fullAccessHost.ts:29
const REMOVE_CWD_FROM_SYS_PATH: [&str; 5] = [
    "import os, os.path, sys",
    "normalize = lambda p: os.path.normcase(os.path.normpath(p))",
    "cwd = normalize(os.getcwd())",
    "orig_sys_path = [p for p in sys.path if p != \"\"]",
    "sys.path[:] = [p for p in sys.path if p != \"\" and normalize(p) != cwd]",
];

// common/fullAccessHost.ts:37
fn extract_sys() -> String {
    let mut parts: Vec<&str> = REMOVE_CWD_FROM_SYS_PATH.to_vec();
    parts.push("import sys, json");
    parts.push("json.dump(dict(path=orig_sys_path, prefix=sys.prefix), sys.stdout)");
    parts.join("; ")
}

// common/fullAccessHost.ts:43
fn extract_version() -> String {
    let mut parts: Vec<&str> = REMOVE_CWD_FROM_SYS_PATH.to_vec();
    parts.push("import sys, json");
    parts.push("json.dump(tuple(sys.version_info), sys.stdout)");
    parts.join("; ")
}

/// Why `execFileSync` threw.
enum ExecError {
    /// The executable could not be started (`ENOENT` with no exit status).
    UnusableCwdOrMissing,
    Failed,
}

// common/fullAccessHost.ts:82
pub struct FullAccessHost {
    fs: &'static dyn FileSystem,
    case_detector: &'static dyn CaseSensitivityDetector,
}

impl FullAccessHost {
    pub fn new(
        fs: &'static dyn FileSystem,
        case_detector: &'static dyn CaseSensitivityDetector,
    ) -> FullAccessHost {
        FullAccessHost { fs, case_detector }
    }

    // common/fullAccessHost.ts:293
    fn get_usable_cwd_path(&self, cwd: Option<Uri>) -> Option<String> {
        get_usable_uri_path(self.fs, cwd)
    }

    // common/fullAccessHost.ts:297
    fn execute_python_interpreter<T>(
        &self,
        python_path: Option<&str>,
        execute: impl Fn(&str) -> Result<Option<T>, ()>,
    ) -> Result<Option<T>, ()> {
        if let Some(python_path) = python_path.filter(|p| !p.is_empty()) {
            execute(python_path)
        } else {
            // A throw from `python3` is swallowed; one from `python` propagates.
            if let Ok(Some(result)) = execute("python3") {
                return Ok(Some(result));
            }

            execute("python")
        }
    }

    // common/fullAccessHost.ts:331
    fn execute_code_in_interpreter(
        &self,
        interpreter_path: &str,
        command_line_args: &[&str],
        code: &str,
        cwd: Option<Uri>,
    ) -> Result<String, ()> {
        let mut args: Vec<&str> = command_line_args.to_vec();
        args.push("-c");
        args.push(code);

        let cwd_path = self.get_usable_cwd_path(cwd);
        let run = |cwd_to_use: Option<&str>| -> Result<String, ExecError> {
            let mut command = Command::new(interpreter_path);
            command
                .args(&args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            if let Some(dir) = cwd_to_use {
                command.current_dir(dir);
            }
            // Miri cannot spawn a process: the interpreter is missing, as on a machine without Python.
            #[cfg(miri)]
            let output: std::io::Result<std::process::Output> = Err(std::io::ErrorKind::NotFound.into());
            #[cfg(not(miri))]
            let output = command.output();
            let output = output.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ExecError::UnusableCwdOrMissing
                } else {
                    ExecError::Failed
                }
            })?;
            if !output.status.success() {
                return Err(ExecError::Failed);
            }
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        };

        match cwd_path {
            None => run(None).map_err(|_| ()),
            Some(cwd_path) => match run(Some(&cwd_path)) {
                Ok(output) => Ok(output),
                Err(ExecError::UnusableCwdOrMissing) => run(None).map_err(|_| ()),
                Err(ExecError::Failed) => Err(()),
            },
        }
    }

    // common/fullAccessHost.ts:376
    fn get_search_path_result_from_interpreter(
        &self,
        interpreter_path: &str,
        import_logger: Option<&ImportLogger>,
        cwd: Option<Uri>,
    ) -> Option<PythonPathResult> {
        let mut result = PythonPathResult {
            paths: Vec::new(),
            prefix: None,
        };

        if let Some(logger) = import_logger {
            logger.log(&format!("Executing interpreter: '{interpreter_path}'"));
        }
        let exec_output = self
            .execute_code_in_interpreter(interpreter_path, &[], &extract_sys(), cwd)
            .ok()?;

        let parsed: Option<Value> = serde_json::from_str(&exec_output).ok();
        let paths = parsed
            .as_ref()
            .and_then(|v| v.get("path"))
            .and_then(Value::as_array);
        let (Some(parsed), Some(paths)) = (parsed.as_ref(), paths) else {
            if let Some(logger) = import_logger {
                logger.log(&format!("Could not parse output: '{exec_output}'"));
            }
            return None;
        };

        for exec_split_entry in paths {
            let Some(entry) = exec_split_entry.as_str() else {
                if let Some(logger) = import_logger {
                    logger.log(&format!("Could not parse output: '{exec_output}'"));
                }
                return None;
            };
            let entry = entry.js_trim();
            if !entry.is_empty() {
                let normalized_path = normalize_path(entry);
                let normalized_uri = Uri::file(&normalized_path, self.case_detector, false);
                if is_usable_directory(self.fs, normalized_uri) {
                    result.paths.push(normalized_uri);
                } else if let Some(logger) = import_logger {
                    logger.log(&format!(
                        "Skipping '{normalized_path}' because it is not a valid directory"
                    ));
                }
            }
        }

        let Some(prefix) = parsed.get("prefix").and_then(Value::as_str) else {
            if let Some(logger) = import_logger {
                logger.log(&format!("Could not parse output: '{exec_output}'"));
            }
            return None;
        };
        result.prefix = Some(Uri::file(prefix, self.case_detector, false));

        if result.paths.is_empty() {
            if let Some(logger) = import_logger {
                logger.log("Found no valid directories");
            }
        }

        Some(result)
    }
}

impl Host for FullAccessHost {
    fn kind(&self) -> HostKind {
        HostKind::FullAccess
    }

    // common/fullAccessHost.ts:104
    fn get_python_search_paths(
        &self,
        python_path: Option<Uri>,
        import_logger: Option<&ImportLogger>,
        cwd: Option<Uri>,
    ) -> PythonPathResult {
        let result = self
            .execute_python_interpreter(python_path.map(|p| p.get_file_path()), |p| {
                Ok(self.get_search_path_result_from_interpreter(p, import_logger, cwd))
            })
            .ok()
            .flatten()
            .unwrap_or(PythonPathResult {
                paths: Vec::new(),
                prefix: None,
            });

        if let Some(logger) = import_logger {
            logger.log(&format!(
                "Received {} paths from interpreter",
                result.paths.len()
            ));
            for path in &result.paths {
                logger.log(&format!("  {}", path.to_uri_string()));
            }
        }
        result
    }

    // common/fullAccessHost.ts:124
    fn get_python_version(
        &self,
        python_path: Option<Uri>,
        import_logger: Option<&ImportLogger>,
    ) -> Option<PythonVersion> {
        let exec_output =
            self.execute_python_interpreter(python_path.map(|p| p.get_file_path()), |p| {
                self.execute_code_in_interpreter(p, &["-I"], &extract_version(), None)
                    .map(Some)
            });
        let Ok(Some(exec_output)) = exec_output else {
            if let Some(logger) = import_logger {
                logger.log("Unable to get Python version from interpreter");
            }
            return None;
        };

        let version_json: Value = match serde_json::from_str(&exec_output) {
            Ok(value) => value,
            Err(_) => {
                if let Some(logger) = import_logger {
                    logger.log("Unable to get Python version from interpreter");
                }
                return None;
            }
        };
        let Some(items) = version_json.as_array().filter(|a| a.len() >= 5) else {
            if let Some(logger) = import_logger {
                logger.log(&format!(
                    "Python version {exec_output} from interpreter is unexpected format"
                ));
            }
            return None;
        };

        let int = |v: &Value| v.as_f64().map(|f| f as i32);
        Some(PythonVersion::create(
            int(&items[0]).unwrap_or(0),
            int(&items[1]).unwrap_or(0),
            int(&items[2]),
            items[3].as_str().and_then(PythonReleaseLevel::from_name),
            int(&items[4]),
        ))
    }

    // common/fullAccessHost.ts:67
    fn get_python_platform(&self, _import_logger: Option<&ImportLogger>) -> Option<PythonPlatform> {
        match std::env::consts::OS {
            "macos" => Some(PythonPlatform::Darwin),
            "linux" => Some(PythonPlatform::Linux),
            "windows" => Some(PythonPlatform::Windows),
            "android" => Some(PythonPlatform::Android),
            _ => None,
        }
    }
}
