//! Port of `packages/pyright-internal/src/common/console.ts`.
//!
//! Node's `console.log` and `console.info` write to stdout; `console.warn` and
//! `console.error` write to stderr. `ConsoleWithLogLevel` and the chaining
//! interfaces serve the language server and are not ported.

use jsrt::cell::Cell;
use std::io::Write;

// common/console.ts:16
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Log,
}

// common/console.ts:22
pub trait ConsoleInterface {
    fn error(&self, message: &str);
    fn warn(&self, message: &str);
    fn info(&self, message: &str);
    fn log(&self, message: &str);
}

// common/console.ts:46
pub fn get_level_number(level: LogLevel) -> i32 {
    match level {
        LogLevel::Error => 0,
        LogLevel::Warn => 1,
        LogLevel::Info => 2,
        LogLevel::Log => 3,
    }
}

/// Node `console.log` / `console.info`.
pub fn node_stdout(message: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{message}");
}

/// Node `console.warn` / `console.error`.
pub fn node_stderr(message: &str) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "{message}");
}

// common/console.ts:53
#[derive(Default)]
pub struct NullConsole {
    pub log_count: Cell<u32>,
    pub info_count: Cell<u32>,
    pub warn_count: Cell<u32>,
    pub error_count: Cell<u32>,
}

impl ConsoleInterface for NullConsole {
    fn log(&self, _message: &str) {
        self.log_count.set(self.log_count.get() + 1);
    }

    fn info(&self, _message: &str) {
        self.info_count.set(self.info_count.get() + 1);
    }

    fn warn(&self, _message: &str) {
        self.warn_count.set(self.warn_count.get() + 1);
    }

    fn error(&self, _message: &str) {
        self.error_count.set(self.error_count.get() + 1);
    }
}

// common/console.ts:76
pub struct StandardConsole {
    max_level: LogLevel,
}

impl StandardConsole {
    pub fn new(max_level: LogLevel) -> StandardConsole {
        StandardConsole { max_level }
    }

    fn enabled(&self, level: LogLevel) -> bool {
        get_level_number(self.max_level) >= get_level_number(level)
    }
}

impl ConsoleInterface for StandardConsole {
    fn log(&self, message: &str) {
        if self.enabled(LogLevel::Log) {
            node_stdout(message);
        }
    }

    fn info(&self, message: &str) {
        if self.enabled(LogLevel::Info) {
            node_stdout(message);
        }
    }

    fn warn(&self, message: &str) {
        if self.enabled(LogLevel::Warn) {
            node_stderr(message);
        }
    }

    fn error(&self, message: &str) {
        if self.enabled(LogLevel::Error) {
            node_stderr(message);
        }
    }
}

// common/console.ts:108
pub struct StderrConsole {
    max_level: LogLevel,
}

impl StderrConsole {
    pub fn new(max_level: LogLevel) -> StderrConsole {
        StderrConsole { max_level }
    }

    fn enabled(&self, level: LogLevel) -> bool {
        get_level_number(self.max_level) >= get_level_number(level)
    }
}

impl ConsoleInterface for StderrConsole {
    fn log(&self, message: &str) {
        if self.enabled(LogLevel::Log) {
            node_stderr(message);
        }
    }

    fn info(&self, message: &str) {
        if self.enabled(LogLevel::Info) {
            node_stderr(message);
        }
    }

    fn warn(&self, message: &str) {
        if self.enabled(LogLevel::Warn) {
            node_stderr(message);
        }
    }

    fn error(&self, message: &str) {
        if self.enabled(LogLevel::Error) {
            node_stderr(message);
        }
    }
}

// common/console.ts:267
pub fn log(console: &dyn ConsoleInterface, log_type: LogLevel, msg: &str) {
    match log_type {
        LogLevel::Log => console.log(msg),
        LogLevel::Info => console.info(msg),
        LogLevel::Warn => console.warn(msg),
        LogLevel::Error => console.error(msg),
    }
}
