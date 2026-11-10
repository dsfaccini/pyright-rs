//! Port of `packages/pyright-internal/src/analyzer/analysis.ts`.

use std::time::Instant;

use crate::analyzer::program::{MaxAnalysisTime, Program, RequiringAnalysisCount};
use crate::common::config_options::ConfigOptions;
use crate::common::diagnostic_sink::FileDiagnostics;

// analyzer/analysis.ts:23
pub struct AnalysisResults {
    pub diagnostics: Vec<FileDiagnostics>,
    pub files_in_program: usize,
    pub checking_only_open_files: bool,
    pub requiring_analysis_count: RequiringAnalysisCount,
    pub fatal_error_occurred: bool,
    pub config_parse_error_occurred: bool,
    pub elapsed_time: f64,
    pub reason: AnalysisReason,
}

/// `'analysis' | 'tracking'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalysisReason {
    Analysis,
    Tracking,
}

// analyzer/analysis.ts:40
pub type AnalysisCompleteCallback = Box<dyn FnMut(AnalysisResults)>;

// analyzer/analysis.ts:42
/// The CLI never cancels, and no analysis error is caught: a Rust panic is a
/// bug, not pyright's "Error performing analysis" path.
pub fn analyze_program(
    program: &Program,
    max_time: Option<MaxAnalysisTime>,
    config_options: &ConfigOptions,
    callback: Option<&mut AnalysisCompleteCallback>,
) -> bool {
    let duration = Instant::now();
    let more_to_analyze = program.analyze(max_time);

    let requiring_analysis_count = program.get_files_to_analyze_count();

    // If we're using command-line mode, the maxTime will be undefined, and we'll
    // want to report all diagnostics rather than just the ones that have changed.
    let report_diagnostic_deltas_only = max_time.is_some();

    let diagnostics = program.get_diagnostics(config_options, report_diagnostic_deltas_only);
    let diagnostic_file_count = diagnostics.len();
    // `Duration` counts whole milliseconds, as `Date.now()` does.
    let elapsed_time = duration.elapsed().as_millis() as f64 / 1000.0;

    // Report any diagnostics or completion.
    if diagnostic_file_count > 0 || !more_to_analyze {
        if let Some(callback) = callback {
            callback(AnalysisResults {
                diagnostics,
                files_in_program: program.get_file_count(true),
                requiring_analysis_count,
                checking_only_open_files: program.is_checking_only_open_files(),
                fatal_error_occurred: false,
                config_parse_error_occurred: false,
                elapsed_time,
                reason: AnalysisReason::Analysis,
            });
        }
    }

    more_to_analyze
}
