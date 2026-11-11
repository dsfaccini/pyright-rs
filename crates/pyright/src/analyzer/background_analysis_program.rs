//! Port of `packages/pyright-internal/src/analyzer/backgroundAnalysisProgram.ts`.
//!
//! The CLI runs analysis in the foreground, so the `IBackgroundAnalysis`
//! branches are not ported.

use jsrt::cell::Cell;

use jsrt::P;

use crate::analyzer::analysis::{AnalysisCompleteCallback, AnalysisReason, AnalysisResults, analyze_program};
use crate::analyzer::import_resolver::ImportResolver;
use crate::analyzer::program::{MaxAnalysisTime, Program};
use crate::common::config_options::ConfigOptions;
use crate::common::diagnostic_sink::FileDiagnostics;
use crate::common::host::Host;
use crate::common::service_provider::ServiceProvider;
use crate::common::uri::Uri;

// analyzer/backgroundAnalysisProgram.ts:32
pub struct BackgroundAnalysisProgram {
    program: P<Program>,
    on_analysis_completion: std::cell::RefCell<Option<AnalysisCompleteCallback>>,
    service_provider: ServiceProvider,
    config_options: Cell<P<ConfigOptions>>,
    import_resolver: Cell<P<ImportResolver>>,
    max_analysis_time: Option<MaxAnalysisTime>,
}

impl BackgroundAnalysisProgram {
    // analyzer/backgroundAnalysisProgram.ts:38
    pub fn new(
        service_provider: ServiceProvider,
        config_options: P<ConfigOptions>,
        import_resolver: P<ImportResolver>,
        max_analysis_time: Option<MaxAnalysisTime>,
        disable_checker: bool,
    ) -> BackgroundAnalysisProgram {
        BackgroundAnalysisProgram {
            program: Program::new(import_resolver, config_options, service_provider, disable_checker),
            on_analysis_completion: std::cell::RefCell::new(None),
            service_provider,
            config_options: Cell::new(config_options),
            import_resolver: Cell::new(import_resolver),
            max_analysis_time,
        }
    }

    // analyzer/backgroundAnalysisProgram.ts:61
    pub fn service_provider(&self) -> ServiceProvider {
        self.service_provider
    }

    // analyzer/backgroundAnalysisProgram.ts:65
    pub fn config_options(&self) -> P<ConfigOptions> {
        self.config_options.get()
    }

    // analyzer/backgroundAnalysisProgram.ts:69
    pub fn import_resolver(&self) -> P<ImportResolver> {
        self.import_resolver.get()
    }

    // analyzer/backgroundAnalysisProgram.ts:73
    pub fn program(&self) -> P<Program> {
        self.program
    }

    // analyzer/backgroundAnalysisProgram.ts:77
    pub fn host(&self) -> &'static dyn Host {
        self.import_resolver.get().host
    }

    // analyzer/backgroundAnalysisProgram.ts:89
    pub fn set_config_options(&self, config_options: P<ConfigOptions>) {
        self.config_options.set(config_options);
        self.program.set_config_options(config_options);
    }

    // analyzer/backgroundAnalysisProgram.ts:95
    pub fn set_import_resolver(&self, import_resolver: P<ImportResolver>) {
        self.import_resolver.set(import_resolver);

        self.program.set_import_resolver(import_resolver);
        for e in self.config_options().get_execution_environments() {
            import_resolver.ensure_partial_stub_packages(&e);
        }
    }

    // analyzer/backgroundAnalysisProgram.ts:103
    pub fn set_tracked_files(&self, file_uris: &[Uri]) {
        let diagnostics = self.program.set_tracked_files(file_uris);
        self.report_diagnostics_for_removed_files(diagnostics);
    }

    // analyzer/backgroundAnalysisProgram.ts:109
    pub fn set_allowed_third_party_imports(&self, import_names: Vec<String>) {
        self.program.set_allowed_third_party_imports(import_names);
    }

    // analyzer/backgroundAnalysisProgram.ts:145
    pub fn mark_all_files_dirty(&self, even_if_contents_are_same: bool) {
        self.program.mark_all_files_dirty(even_if_contents_are_same);
    }

    // analyzer/backgroundAnalysisProgram.ts:155
    pub fn set_completion_callback(&self, callback: Option<AnalysisCompleteCallback>) {
        *self.on_analysis_completion.borrow_mut() = callback;
    }

    // analyzer/backgroundAnalysisProgram.ts:160
    pub fn start_analysis(&self) -> bool {
        // The callback runs outside the borrow, so it may call back into this object.
        let mut callback = self.on_analysis_completion.borrow_mut().take();
        let more_to_analyze =
            analyze_program(&self.program, self.max_analysis_time, &self.config_options(), callback.as_mut());
        self.restore_callback(callback);
        more_to_analyze
    }

    /// Puts the callback back unless the call replaced it.
    fn restore_callback(&self, callback: Option<AnalysisCompleteCallback>) {
        let mut slot = self.on_analysis_completion.borrow_mut();
        if slot.is_none() {
            *slot = callback;
        }
    }

    // analyzer/backgroundAnalysisProgram.ts:273
    fn report_diagnostics_for_removed_files(&self, file_diags: Vec<FileDiagnostics>) {
        if file_diags.is_empty() {
            return;
        }

        // If analysis is running in the foreground process, report any
        // diagnostics that resulted from the close operation.
        let mut callback = self.on_analysis_completion.borrow_mut().take();
        if let Some(callback) = callback.as_mut() {
            callback(AnalysisResults {
                diagnostics: file_diags,
                files_in_program: self.program.get_file_count(true),
                requiring_analysis_count: self.program.get_files_to_analyze_count(),
                checking_only_open_files: self.program.is_checking_only_open_files(),
                fatal_error_occurred: false,
                config_parse_error_occurred: false,
                elapsed_time: 0.0,
                reason: AnalysisReason::Tracking,
            });
        }
        self.restore_callback(callback);
    }

    /// Calls the completion callback directly, for the service's config parse error report.
    pub fn call_completion_callback(&self, results: AnalysisResults) -> bool {
        let mut callback = self.on_analysis_completion.borrow_mut().take();
        let called = callback.as_mut().map(|callback| callback(results)).is_some();
        self.restore_callback(callback);
        called
    }
}
