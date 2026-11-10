//! The services pyright looks up through `ServiceProvider` and
//! `serviceProviderExtensions.ts` on the CLI path, as plain fields.
//! Ports `common/serviceProvider.ts` only as far as the check uses it.

use std::cell::OnceCell;

use jsrt::P;

use crate::common::case_sensitivity_detector::CaseSensitivityDetector;
use crate::common::console::ConsoleInterface;
use crate::common::file_system::FileSystem;
use crate::partial_stub_service::{PartialStubService, SupportPartialStubs};

// common/serviceProvider.ts:42
/// Every service lives for the whole run, so references are `'static`.
#[derive(Clone, Copy)]
#[derive(jsrt::Trace)]
pub struct ServiceProvider {
    pub fs: &'static dyn FileSystem,
    pub console: &'static dyn ConsoleInterface,
    pub case_sensitivity_detector: &'static dyn CaseSensitivityDetector,
    /// `ServiceKeys.partialStubs`: shared by every copy of this provider.
    /// `partial_stubs()` fills it on first use; set it first to register one.
    pub partial_stubs: P<OnceCell<&'static dyn SupportPartialStubs>>,
}

impl ServiceProvider {
    pub fn new(
        fs: &'static dyn FileSystem,
        console: &'static dyn ConsoleInterface,
        case_sensitivity_detector: &'static dyn CaseSensitivityDetector,
    ) -> ServiceProvider {
        ServiceProvider { fs, console, case_sensitivity_detector, partial_stubs: P::new(OnceCell::new()) }
    }

    pub fn fs(&self) -> &'static dyn FileSystem {
        self.fs
    }

    pub fn console(&self) -> &'static dyn ConsoleInterface {
        self.console
    }

    // common/serviceProviderExtensions.ts:30
    pub fn partial_stubs(&self) -> &'static dyn SupportPartialStubs {
        *self.partial_stubs.get().get_or_init(|| P::new(PartialStubService::new(self.fs())).get())
    }
}
