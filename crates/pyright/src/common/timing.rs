//! Port of `packages/pyright-internal/src/common/timing.ts`.

use jsrt::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

use jsrt::number::number_to_string;

use crate::common::console::ConsoleInterface;

/// `Date.now()`: whole milliseconds since the epoch.
fn date_now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// JS `Math.round`: halves round toward +Infinity.
fn math_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

// common/timing.ts:13
pub struct Duration {
    start_time: f64,
}

impl Default for Duration {
    fn default() -> Self {
        Duration::new()
    }
}

impl Duration {
    // common/timing.ts:16
    pub fn new() -> Duration {
        Duration { start_time: date_now() }
    }

    // common/timing.ts:20
    pub fn get_duration_in_milliseconds(&self) -> f64 {
        let cur_time = date_now();
        cur_time - self.start_time
    }

    // common/timing.ts:25
    pub fn get_duration_in_seconds(&self) -> f64 {
        self.get_duration_in_milliseconds() / 1000.0
    }
}

// common/timing.ts:30
#[derive(Default)]
pub struct TimingStat {
    pub total_time: Cell<f64>,
    pub call_count: Cell<u64>,
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
            self.total_time.set(self.total_time.get() + duration.get_duration_in_milliseconds());
            self.is_timing.set(false);

            result
        }
    }

    // common/timing.ts:52
    pub fn subtract_from_time(&self, callback: impl FnOnce()) {
        if self.is_timing.get() {
            self.is_timing.set(false);
            let duration = Duration::new();
            callback();
            self.total_time.set(self.total_time.get() - duration.get_duration_in_milliseconds());
            self.is_timing.set(true);
        } else {
            callback();
        }
    }

    // common/timing.ts:64
    pub fn print_time(&self) -> String {
        let total_time_in_sec = self.total_time.get() / 1000.0;
        let rounded_time = math_round(total_time_in_sec * 100.0) / 100.0;
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
        console.info(&format!("Completed in {}sec", number_to_string(self.total_duration.get_duration_in_seconds())));
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
    // common/timing.ts:105
    static TIMING_STATS: &'static TimingStats = Box::leak(Box::default());
}

/// `timingStats`, the module-level singleton. pyright creates it when
/// `pyright.ts` loads, before anything else; the CLI touches it first thing.
pub fn timing_stats() -> &'static TimingStats {
    TIMING_STATS.with(|t| *t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_time_rounds_to_hundredths_like_js() {
        let stat = TimingStat::default();
        stat.total_time.set(1234.0);
        assert_eq!(stat.print_time(), "1.23sec");
        stat.total_time.set(1235.0);
        assert_eq!(stat.print_time(), "1.24sec");
        stat.total_time.set(0.0);
        assert_eq!(stat.print_time(), "0sec");
        stat.total_time.set(-4.0);
        assert_eq!(stat.print_time(), "0sec");
        assert_eq!(math_round(-1.5), -1.0);
        assert_eq!(math_round(0.49999999999999994), 0.0);
    }

    #[test]
    fn reentrant_operations_count_but_time_once() {
        let stat = TimingStat::default();
        stat.time_operation(|| stat.time_operation(|| ()));
        assert_eq!(stat.call_count.get(), 2);
        assert!(!stat.is_timing.get());
    }
}
