//! The services pyright looks up through `ServiceProvider` and
//! `serviceProviderExtensions.ts` on the CLI path, as plain fields.
//! Ports `common/serviceProvider.ts` only as far as the check uses it.

use crate::common::case_sensitivity_detector::CaseSensitivityDetector;
use crate::common::console::ConsoleInterface;
use crate::common::file_system::FileSystem;

// common/serviceProvider.ts:42
/// Every service lives for the whole run, so references are `'static`.
#[derive(Clone, Copy)]
pub struct ServiceProvider {
    pub fs: &'static dyn FileSystem,
    pub console: &'static dyn ConsoleInterface,
    pub case_sensitivity_detector: &'static dyn CaseSensitivityDetector,
}

impl ServiceProvider {
    pub fn fs(&self) -> &'static dyn FileSystem {
        self.fs
    }

    pub fn console(&self) -> &'static dyn ConsoleInterface {
        self.console
    }
}
