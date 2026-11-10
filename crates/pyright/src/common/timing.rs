//! Port of `packages/pyright-internal/src/common/timing.ts`.
//!
//! Durations are fractional milliseconds from a monotonic clock. Pyright's
//! `Date.now()` differences are whole milliseconds; truncating each short
//! operation to whole milliseconds here would sum to zero. No timing reaches
//! `--outputjson`.

use std::cell::Cell;
use std::time::Instant;

use jsrt::P;
use jsrt::number::number_to_string;

use crate::common::console::ConsoleInterface;

// common/timing.ts:13
pub struct Duration {
    _start_time: Instant,
}

impl Duration {
    // common/timing.ts:16
    pub fn new() -> Duration {
        Duration { _start_time: Instant::now() }
    }

    // common/timing.ts:20
    pub fn get_duration_in_milliseconds(&self) -> f64 {
        self._start_time.elapsed().as_secs_f64() * 1000.0
    }

    // common/timing.ts:25
    pub fn get_duration_in_seconds(&self) -> f64 {
        self.get_duration_in_milliseconds() / 1000.0
    }
}

impl Default for Duration {
    fn default() -> Self {
        Duration::new()
    }
}

// common/timing.ts:30
#[derive(Default)]
pub struct TimingStat {
    pub total_time: Cell<f64>,
    pub call_count: Cell<u32>,
    pub is_timing: Cell<bool>,
}

impl TimingStat {
    // common/timing.ts:35
    pub fn time_operation<R>(&self, callback: impl FnOnce() -> R) -> R {
        self.call_count.set(self.call_count.get() + 1);

        // Handle reentrancy.
        if self.is_timing.get() {
            callback()
        } else {
            self.is_timing.set(true);
            let duration = Duration::new();
            let result = callback();
            self.total_time
                .set(self.total_time.get() + duration.get_duration_in_milliseconds());
            self.is_timing.set(false);

            result
        }
    }

    // common/timing.ts:52
    pub fn subtract_from_time<R>(&self, callback: impl FnOnce() -> R) -> R {
        if self.is_timing.get() {
            self.is_timing.set(false);
            let duration = Duration::new();
            let result = callback();
            self.total_time
                .set(self.total_time.get() - duration.get_duration_in_milliseconds());
            self.is_timing.set(true);
            result
        } else {
            callback()
        }
    }

    // common/timing.ts:64
    pub fn print_time(&self) -> String {
        let total_time_in_sec = self.total_time.get() / 1000.0;
        // `Math.round`: halves round up.
        let rounded_time = (total_time_in_sec * 100.0 + 0.5).floor() / 100.0;
        number_to_string(rounded_time) + "sec"
    }
}

// common/timing.ts:71
#[derive(Default)]
pub struct TimingStats {
    pub total_duration: Duration,
    pub find_files_time: TimingStat,
    pub read_file_time: TimingStat,
    pub tokenize_file_time: TimingStat,
    pub parse_file_time: TimingStat,
    pub resolve_imports_time: TimingStat,
    pub cycle_detection_time: TimingStat,
    pub bind_time: TimingStat,
    pub type_checker_time: TimingStat,
    pub type_evaluation_time: TimingStat,
}

impl TimingStats {
    // common/timing.ts:83
    pub fn print_summary(&self, console: &dyn ConsoleInterface) {
        console.info(&format!(
            "Completed in {}sec",
            number_to_string(self.total_duration.get_duration_in_seconds())
        ));
    }

    // common/timing.ts:87
    pub fn print_details(&self, console: &dyn ConsoleInterface) {
        console.info("");
        console.info("Timing stats");
        console.info(&format!("Find Source Files:    {}", self.find_files_time.print_time()));
        console.info(&format!("Read Source Files:    {}", self.read_file_time.print_time()));
        console.info(&format!("Tokenize:             {}", self.tokenize_file_time.print_time()));
        console.info(&format!("Parse:                {}", self.parse_file_time.print_time()));
        console.info(&format!("Resolve Imports:      {}", self.resolve_imports_time.print_time()));
        console.info(&format!("Bind:                 {}", self.bind_time.print_time()));
        console.info(&format!("Check:                {}", self.type_checker_time.print_time()));
        console.info(&format!("Detect Cycles:        {}", self.cycle_detection_time.print_time()));
    }

    // common/timing.ts:100
    pub fn get_total_duration(&self) -> f64 {
        self.total_duration.get_duration_in_seconds()
    }
}

thread_local! {
    static TIMING_STATS: P<TimingStats> = P::new(TimingStats::default());
}

// common/timing.ts:105
/// The module-level `timingStats` singleton, one per thread.
pub fn timing_stats() -> P<TimingStats> {
    TIMING_STATS.with(|stats| *stats)
}
