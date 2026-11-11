//! Port of `packages/pyright-internal/src/common/diagnosticSink.ts`.

use jsrt::cell::RefCell;

use jsrt::{JsMap, P};

use crate::common::config_options::DiagnosticLevel;
use crate::common::diagnostic::{Diagnostic, DiagnosticAction, DiagnosticCategory};
use crate::common::position_utils::convert_offsets_to_range;
use crate::common::string_utils::hash_string;
use crate::common::text_range::{Range, TextRange};
use crate::common::text_range_collection::TextRangeCollection;
use crate::common::uri::Uri;

// common/diagnosticSink.ts:20
#[derive(Clone, Debug)]
pub struct FileDiagnostics {
    pub file_uri: Uri,
    pub version: Option<i32>,
    pub diagnostics: Vec<P<Diagnostic>>,
}

// common/diagnosticSink.ts:45
#[derive(Default)]
#[derive(jsrt::Trace)]
pub struct DiagnosticSink {
    diagnostic_list: RefCell<Vec<P<Diagnostic>>>,
    diagnostic_map: RefCell<JsMap<String, P<Diagnostic>>>,
}

impl DiagnosticSink {
    // common/diagnosticSink.ts:49
    pub fn new(diagnostics: Option<Vec<P<Diagnostic>>>) -> DiagnosticSink {
        DiagnosticSink {
            diagnostic_list: RefCell::new(diagnostics.unwrap_or_default()),
            diagnostic_map: Default::default(),
        }
    }

    // common/diagnosticSink.ts:54
    pub fn fetch_and_clear(&self) -> Vec<P<Diagnostic>> {
        let prev_diagnostics = std::mem::take(&mut *self.diagnostic_list.borrow_mut());
        self.diagnostic_map.borrow_mut().clear();
        prev_diagnostics
    }

    // common/diagnosticSink.ts:61
    pub fn add_error(&self, message: String, range: Range) -> P<Diagnostic> {
        self.add_diagnostic(Diagnostic::new(DiagnosticCategory::Error, message, range))
    }

    // common/diagnosticSink.ts:65
    pub fn add_warning(&self, message: String, range: Range) -> P<Diagnostic> {
        self.add_diagnostic(Diagnostic::new(DiagnosticCategory::Warning, message, range))
    }

    // common/diagnosticSink.ts:69
    pub fn add_information(&self, message: String, range: Range) -> P<Diagnostic> {
        self.add_diagnostic(Diagnostic::new(
            DiagnosticCategory::Information,
            message,
            range,
        ))
    }

    // common/diagnosticSink.ts:73
    pub fn add_unused_code(
        &self,
        message: String,
        range: Range,
        action: Option<DiagnosticAction>,
    ) -> P<Diagnostic> {
        let diag = Diagnostic::new(DiagnosticCategory::UnusedCode, message, range);
        if let Some(action) = action {
            diag.add_action(action);
        }
        self.add_diagnostic(diag)
    }

    // common/diagnosticSink.ts:81
    pub fn add_unreachable_code(
        &self,
        message: String,
        range: Range,
        action: Option<DiagnosticAction>,
    ) -> P<Diagnostic> {
        let diag = Diagnostic::new(DiagnosticCategory::UnreachableCode, message, range);
        if let Some(action) = action {
            diag.add_action(action);
        }
        self.add_diagnostic(diag)
    }

    // common/diagnosticSink.ts:89
    pub fn add_deprecated(
        &self,
        message: String,
        range: Range,
        action: Option<DiagnosticAction>,
    ) -> P<Diagnostic> {
        let diag = Diagnostic::new(DiagnosticCategory::Deprecated, message, range);
        if let Some(action) = action {
            diag.add_action(action);
        }
        self.add_diagnostic(diag)
    }

    // common/diagnosticSink.ts:97
    pub fn add_diagnostic(&self, diag: P<Diagnostic>) -> P<Diagnostic> {
        // The stray `}` at the end of the key is in pyright's template.
        let key = format!(
            "{},{}-{}-{}:{}}}",
            diag.range.start.line,
            diag.range.start.character,
            diag.range.end.line,
            diag.range.end.character,
            hash_string(&diag.message)
        );
        let mut map = self.diagnostic_map.borrow_mut();
        if !map.contains_key(&key) {
            self.diagnostic_list.borrow_mut().push(diag);
            map.insert(key, diag);
        }
        diag
    }

    // common/diagnosticSink.ts:110
    pub fn add_diagnostics(&self, diags_to_add: &[P<Diagnostic>]) {
        self.diagnostic_list
            .borrow_mut()
            .extend_from_slice(diags_to_add);
    }

    fn filter_category(&self, category: DiagnosticCategory) -> Vec<P<Diagnostic>> {
        self.diagnostic_list
            .borrow()
            .iter()
            .copied()
            .filter(|diag| diag.category == category)
            .collect()
    }

    // common/diagnosticSink.ts:114
    pub fn get_errors(&self) -> Vec<P<Diagnostic>> {
        self.filter_category(DiagnosticCategory::Error)
    }

    // common/diagnosticSink.ts:118
    pub fn get_warnings(&self) -> Vec<P<Diagnostic>> {
        self.filter_category(DiagnosticCategory::Warning)
    }

    // common/diagnosticSink.ts:122
    pub fn get_information(&self) -> Vec<P<Diagnostic>> {
        self.filter_category(DiagnosticCategory::Information)
    }

    // common/diagnosticSink.ts:126
    pub fn get_unused_code(&self) -> Vec<P<Diagnostic>> {
        self.filter_category(DiagnosticCategory::UnusedCode)
    }

    // common/diagnosticSink.ts:130
    pub fn get_unreachable_code(&self) -> Vec<P<Diagnostic>> {
        self.filter_category(DiagnosticCategory::UnreachableCode)
    }

    // common/diagnosticSink.ts:134
    pub fn get_deprecated(&self) -> Vec<P<Diagnostic>> {
        self.filter_category(DiagnosticCategory::Deprecated)
    }
}

// common/diagnosticSink.ts:141
/// Derefs to its `DiagnosticSink`, as the TypeScript subclass inherits it.
#[derive(jsrt::Trace)]
pub struct TextRangeDiagnosticSink {
    sink: DiagnosticSink,
    lines: &'static TextRangeCollection<TextRange>,
}

impl std::ops::Deref for TextRangeDiagnosticSink {
    type Target = DiagnosticSink;

    fn deref(&self) -> &DiagnosticSink {
        &self.sink
    }
}

impl TextRangeDiagnosticSink {
    // common/diagnosticSink.ts:144
    pub fn new(
        lines: &'static TextRangeCollection<TextRange>,
        diagnostics: Option<Vec<P<Diagnostic>>>,
    ) -> TextRangeDiagnosticSink {
        TextRangeDiagnosticSink {
            sink: DiagnosticSink::new(diagnostics),
            lines,
        }
    }

    fn to_range(&self, range: TextRange) -> Range {
        convert_offsets_to_range(range.start, range.start + range.length, self.lines)
    }

    // common/diagnosticSink.ts:149
    pub fn add_diagnostic_with_text_range(
        &self,
        level: DiagnosticLevel,
        message: String,
        range: TextRange,
    ) -> P<Diagnostic> {
        let position_range = self.to_range(range);
        match level {
            DiagnosticLevel::Error => self.add_error(message, position_range),
            DiagnosticLevel::Warning => self.add_warning(message, position_range),
            DiagnosticLevel::Information => self.add_information(message, position_range),
            DiagnosticLevel::None => panic!("none is not expected value"),
        }
    }

    // common/diagnosticSink.ts:166
    pub fn add_unused_code_with_text_range(
        &self,
        message: String,
        range: TextRange,
        action: Option<DiagnosticAction>,
    ) -> P<Diagnostic> {
        self.add_unused_code(message, self.to_range(range), action)
    }

    // common/diagnosticSink.ts:174
    pub fn add_unreachable_code_with_text_range(
        &self,
        message: String,
        range: TextRange,
        action: Option<DiagnosticAction>,
    ) -> P<Diagnostic> {
        self.add_unreachable_code(message, self.to_range(range), action)
    }

    // common/diagnosticSink.ts:182
    pub fn add_deprecated_with_text_range(
        &self,
        message: String,
        range: TextRange,
        action: Option<DiagnosticAction>,
    ) -> P<Diagnostic> {
        self.add_deprecated(message, self.to_range(range), action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::text_range::Position;

    fn range(line: i32, start: i32, end: i32) -> Range {
        Range {
            start: Position {
                line,
                character: start,
            },
            end: Position {
                line,
                character: end,
            },
        }
    }

    #[test]
    fn drops_duplicates_by_range_and_message() {
        let sink = DiagnosticSink::new(None);
        let a = sink.add_error("boom".to_string(), range(1, 2, 3));
        let b = sink.add_error("boom".to_string(), range(1, 2, 3));
        sink.add_warning("boom".to_string(), range(1, 2, 4));
        assert_ne!(a, b);
        assert_eq!(sink.get_errors().len(), 1);
        assert_eq!(sink.get_warnings().len(), 1);
        assert_eq!(sink.fetch_and_clear().len(), 2);
        assert!(sink.fetch_and_clear().is_empty());
    }

    #[test]
    fn text_range_sink_converts_offsets() {
        let lines: &'static TextRangeCollection<TextRange> =
            Box::leak(Box::new(TextRangeCollection::new(vec![
                TextRange {
                    start: 0,
                    length: 4,
                },
                TextRange {
                    start: 4,
                    length: 3,
                },
            ])));
        let sink = TextRangeDiagnosticSink::new(lines, None);
        let diag = sink.add_diagnostic_with_text_range(
            DiagnosticLevel::Error,
            "x".to_string(),
            TextRange {
                start: 5,
                length: 1,
            },
        );
        assert_eq!(diag.range, range(1, 1, 2));
    }
}
