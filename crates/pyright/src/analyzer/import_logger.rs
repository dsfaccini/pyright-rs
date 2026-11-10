//! Port of `packages/pyright-internal/src/analyzer/importLogger.ts`.

use std::cell::RefCell;

// analyzer/importLogger.ts:10
#[derive(Default)]
pub struct ImportLogger {
    logs: RefCell<Vec<String>>,
}

impl ImportLogger {
    pub fn new() -> ImportLogger {
        ImportLogger::default()
    }

    // analyzer/importLogger.ts:13
    pub fn log(&self, message: &str) {
        self.logs.borrow_mut().push(message.to_string());
    }

    // analyzer/importLogger.ts:17
    pub fn get_logs(&self) -> Vec<String> {
        self.logs.borrow().clone()
    }
}
