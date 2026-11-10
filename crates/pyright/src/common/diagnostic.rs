//! Port of `packages/pyright-internal/src/common/diagnostic.ts`.
//!
//! JSON (de)serialization (`toJsonObj`, `fromJsonObj`) serves the language
//! server's background threads and is not ported.

use std::cell::RefCell;

use jsrt::P;

use crate::common::config_options::DiagnosticLevel;
use crate::common::text_range::{Range, TextRange};
use crate::common::uri::Uri;

// common/diagnostic.ts:16
pub const DEFAULT_MAX_DIAGNOSTIC_DEPTH: i32 = 5;
// common/diagnostic.ts:17
pub const DEFAULT_MAX_DIAGNOSTIC_LINE_COUNT: usize = 8;
// common/diagnostic.ts:18
const MAX_RECURSION_COUNT: i32 = 64;

// common/diagnostic.ts:21
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskListPriority {
    High,
    Normal,
    Low,
}

impl TaskListPriority {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskListPriority::High => "High",
            TaskListPriority::Normal => "Normal",
            TaskListPriority::Low => "Low",
        }
    }
}

// common/diagnostic.ts:27
#[derive(Clone, Debug)]
pub struct TaskListToken {
    pub text: String,
    pub priority: TaskListPriority,
}

// common/diagnostic.ts:32
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticCategory {
    Error,
    Warning,
    Information,
    UnusedCode,
    UnreachableCode,
    Deprecated,
    TaskItem,
}

// common/diagnostic.ts:42
pub fn convert_level_to_category(level: DiagnosticLevel) -> DiagnosticCategory {
    match level {
        DiagnosticLevel::Error => DiagnosticCategory::Error,
        DiagnosticLevel::Warning => DiagnosticCategory::Warning,
        DiagnosticLevel::Information => DiagnosticCategory::Information,
        DiagnosticLevel::None => panic!("none is not expected"),
    }
}

// common/diagnostic.ts:57
/// `DiagnosticAction` and its one check-path subtype, `CreateTypeStubFileAction`.
#[derive(Clone, Debug)]
pub struct DiagnosticAction {
    pub action: String,
    pub module_name: Option<String>,
}

// common/diagnostic.ts:61
pub struct DiagnosticWithinFile {
    pub uri: Uri,
    pub diagnostic: P<Diagnostic>,
}

// common/diagnostic.ts:71
#[derive(Clone, Debug)]
pub struct DiagnosticRelatedInfo {
    pub message: String,
    pub uri: Uri,
    pub range: Range,
    pub priority: TaskListPriority,
}

// common/diagnostic.ts:97
/// A diagnostic is shared: the sink stores it and the caller sets its rule
/// afterwards. Pass it around as `P<Diagnostic>`.
#[derive(Debug)]
pub struct Diagnostic {
    pub category: DiagnosticCategory,
    pub message: String,
    pub range: Range,
    pub priority: TaskListPriority,
    actions: RefCell<Option<Vec<DiagnosticAction>>>,
    rule: RefCell<Option<String>>,
    related_info: RefCell<Vec<DiagnosticRelatedInfo>>,
}

impl Diagnostic {
    // common/diagnostic.ts:103
    pub fn new(category: DiagnosticCategory, message: String, range: Range) -> P<Diagnostic> {
        Diagnostic::with_priority(category, message, range, TaskListPriority::Normal)
    }

    // common/diagnostic.ts:103
    pub fn with_priority(
        category: DiagnosticCategory,
        message: String,
        range: Range,
        priority: TaskListPriority,
    ) -> P<Diagnostic> {
        P::new(Diagnostic {
            category,
            message,
            range,
            priority,
            actions: RefCell::new(None),
            rule: RefCell::new(None),
            related_info: RefCell::new(Vec::new()),
        })
    }

    // common/diagnostic.ts:133
    pub fn add_action(&self, action: DiagnosticAction) {
        self.actions
            .borrow_mut()
            .get_or_insert_with(Vec::new)
            .push(action);
    }

    // common/diagnostic.ts:149
    pub fn get_actions(&self) -> Option<Vec<DiagnosticAction>> {
        self.actions.borrow().clone()
    }

    // common/diagnostic.ts:153
    pub fn set_rule(&self, rule: &str) {
        *self.rule.borrow_mut() = Some(rule.to_string());
    }

    // common/diagnostic.ts:157
    pub fn get_rule(&self) -> Option<String> {
        self.rule.borrow().clone()
    }

    // common/diagnostic.ts:161
    pub fn add_related_info(
        &self,
        message: String,
        file_uri: Uri,
        range: Range,
        priority: TaskListPriority,
    ) {
        self.related_info.borrow_mut().push(DiagnosticRelatedInfo {
            uri: file_uri,
            message,
            range,
            priority,
        });
    }

    // common/diagnostic.ts:165
    pub fn get_related_info(&self) -> Vec<DiagnosticRelatedInfo> {
        self.related_info.borrow().clone()
    }
}

// common/diagnostic.ts:171
pub fn compare_diagnostics(d1: &Diagnostic, d2: &Diagnostic) -> i32 {
    if d1.range.start.line < d2.range.start.line {
        return -1;
    } else if d1.range.start.line > d2.range.start.line {
        return 1;
    }

    if d1.range.start.character < d2.range.start.character {
        return -1;
    } else if d1.range.start.character > d2.range.start.character {
        return 1;
    }

    0
}

// common/diagnostic.ts:189
/// Shared: `createAddendum` returns a child the caller keeps writing to.
#[derive(Default)]
pub struct DiagnosticAddendum {
    messages: RefCell<Vec<String>>,
    child_addenda: RefCell<Vec<P<DiagnosticAddendum>>>,
    nest_level: std::cell::Cell<Option<i32>>,
    range: std::cell::Cell<Option<TextRange>>,
}

impl DiagnosticAddendum {
    pub fn new() -> P<DiagnosticAddendum> {
        P::new(DiagnosticAddendum::default())
    }

    // common/diagnostic.ts:203
    pub fn add_message(&self, message: String) {
        self.messages.borrow_mut().push(message);
    }

    // common/diagnostic.ts:207
    pub fn add_message_multiline(&self, message: &str) {
        for line in message.split('\n') {
            self.messages.borrow_mut().push(line.to_string());
        }
    }

    // common/diagnostic.ts:213
    pub fn add_text_range(&self, range: TextRange) {
        self.range.set(Some(range));
    }

    // common/diagnostic.ts:218
    pub fn create_addendum(&self) -> P<DiagnosticAddendum> {
        let new_addendum = DiagnosticAddendum::new();
        new_addendum
            .nest_level
            .set(Some(self.nest_level.get().unwrap_or(0) + 1));
        self.add_addendum(new_addendum);
        new_addendum
    }

    // common/diagnostic.ts:225
    pub fn get_string(&self, max_depth: i32, max_line_count: usize) -> String {
        let mut lines = self.get_lines_recursive(max_depth, max_line_count, 0);

        if lines.len() > max_line_count {
            lines.truncate(max_line_count);
            lines.push("  ...".to_string());
        }

        let text = lines.join("\n");
        if !text.is_empty() {
            return format!("\n{text}");
        }

        String::new()
    }

    // common/diagnostic.ts:240
    pub fn is_empty(&self) -> bool {
        self.get_message_count(0) == 0
    }

    // common/diagnostic.ts:244
    pub fn add_addendum(&self, addendum: P<DiagnosticAddendum>) {
        self.child_addenda.borrow_mut().push(addendum);
    }

    // common/diagnostic.ts:248
    pub fn get_children(&self) -> Vec<P<DiagnosticAddendum>> {
        self.child_addenda.borrow().clone()
    }

    // common/diagnostic.ts:252
    pub fn get_messages(&self) -> Vec<String> {
        self.messages.borrow().clone()
    }

    // common/diagnostic.ts:256
    pub fn get_nest_level(&self) -> i32 {
        self.nest_level.get().unwrap_or(0)
    }

    // common/diagnostic.ts:263
    pub fn get_effective_text_range(&self) -> Option<TextRange> {
        let range = self.get_text_range_recursive(0);

        if range.is_some_and(|r| r.length == 0) {
            return None;
        }

        range
    }

    // common/diagnostic.ts:275
    fn get_text_range_recursive(&self, recursion_count: i32) -> Option<TextRange> {
        if recursion_count > MAX_RECURSION_COUNT {
            return None;
        }
        let recursion_count = recursion_count + 1;

        let child_ranges: Vec<TextRange> = self
            .get_children()
            .iter()
            .filter_map(|child| child.get_text_range_recursive(recursion_count))
            .collect();

        if child_ranges.len() > 1 {
            return Some(TextRange {
                start: 0,
                length: 0,
            });
        }

        if child_ranges.len() == 1 {
            return Some(child_ranges[0]);
        }

        self.range.get()
    }

    // common/diagnostic.ts:298
    fn get_message_count(&self, recursion_count: i32) -> usize {
        if recursion_count > MAX_RECURSION_COUNT {
            return 0;
        }

        let mut message_count = self.messages.borrow().len();

        for diag in self.get_children() {
            message_count += diag.get_message_count(recursion_count + 1);
        }

        message_count
    }

    // common/diagnostic.ts:313
    fn get_lines_recursive(
        &self,
        max_depth: i32,
        max_line_count: usize,
        recursion_count: i32,
    ) -> Vec<String> {
        if max_depth <= 0 || recursion_count > MAX_RECURSION_COUNT {
            return Vec::new();
        }

        let messages = self.get_messages();
        let mut child_lines: Vec<String> = Vec::new();
        for addendum in self.get_children() {
            let max_depth_remaining = if !messages.is_empty() {
                max_depth - 1
            } else {
                max_depth
            };
            child_lines.extend(addendum.get_lines_recursive(
                max_depth_remaining,
                max_line_count,
                recursion_count + 1,
            ));

            if child_lines.len() >= max_line_count {
                child_lines.truncate(max_line_count);
                break;
            }
        }

        let extra_space = if !messages.is_empty() { "  " } else { "" };
        messages
            .into_iter()
            .chain(child_lines)
            .map(|line| format!("{extra_space}{line}"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addendum_nests_and_truncates() {
        let root = DiagnosticAddendum::new();
        let child = root.create_addendum();
        child.add_message("first".to_string());
        let grandchild = child.create_addendum();
        grandchild.add_message("second".to_string());
        assert_eq!(
            root.get_string(
                DEFAULT_MAX_DIAGNOSTIC_DEPTH,
                DEFAULT_MAX_DIAGNOSTIC_LINE_COUNT
            ),
            "\n  first\n    second"
        );
        assert_eq!(grandchild.get_nest_level(), 2);

        let wide = DiagnosticAddendum::new();
        for i in 0..10 {
            wide.add_message(format!("m{i}"));
        }
        let text = wide.get_string(
            DEFAULT_MAX_DIAGNOSTIC_DEPTH,
            DEFAULT_MAX_DIAGNOSTIC_LINE_COUNT,
        );
        assert!(text.ends_with("  m7\n  ..."));
    }

    #[test]
    fn effective_range() {
        let root = DiagnosticAddendum::new();
        assert_eq!(root.get_effective_text_range(), None);
        root.create_addendum().add_text_range(TextRange {
            start: 3,
            length: 2,
        });
        assert_eq!(
            root.get_effective_text_range(),
            Some(TextRange {
                start: 3,
                length: 2
            })
        );
        root.create_addendum().add_text_range(TextRange {
            start: 9,
            length: 2,
        });
        assert_eq!(root.get_effective_text_range(), None);
    }
}
