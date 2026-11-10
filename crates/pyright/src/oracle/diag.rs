//! Rust side of the `diag` dump stage. Format: `oracle/FORMAT.md`.

use jsrt::P;

use super::format::{self, DumpWriter};
use crate::common::diagnostic::Diagnostic;
use crate::common::text_range::Range;

/// The `SourceFile` diagnostic lists the diag stage prints, in file order.
pub struct OracleDiagnosticLists<'a> {
    pub parse: &'a [P<Diagnostic>],
    pub comment: &'a [P<Diagnostic>],
    pub bind: &'a [P<Diagnostic>],
    pub checker: &'a [P<Diagnostic>],
    pub task_list: &'a [P<Diagnostic>],
    pub final_: &'a [P<Diagnostic>],
}

/// The Range form: `<start.line>:<start.character>-<end.line>:<end.character>`.
pub fn format_range(range: &Range) -> String {
    format!("{}:{}-{}:{}", range.start.line, range.start.character, range.end.line, range.end.character)
}

/// A whole diag-stage file.
pub fn dump_diagnostics(lists: &OracleDiagnosticLists) -> String {
    let mut w = DumpWriter::new();
    let mut emit_list = |name: &str, diagnostics: &[P<Diagnostic>]| {
        w.field(0, name, &format::array_header(diagnostics.len()));
        for (i, diag) in diagnostics.iter().enumerate() {
            let rule = diag.get_rule().unwrap_or_else(|| "-".to_string());
            w.field(
                1,
                &format::index_key(i),
                &format!(
                    "{} {} {rule} {}",
                    diag.category as i32,
                    format_range(&diag.range),
                    format::str(&diag.message)
                ),
            );
        }
    };

    emit_list("parse", lists.parse);
    emit_list("comment", lists.comment);
    emit_list("bind", lists.bind);
    emit_list("checker", lists.checker);
    emit_list("taskList", lists.task_list);
    emit_list("final", lists.final_);
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::diagnostic::DiagnosticCategory;
    use crate::common::text_range::Position;

    #[test]
    fn matches_the_format_example() {
        let diag = Diagnostic::new(
            DiagnosticCategory::UnusedCode,
            "\"os\" is not accessed".to_string(),
            Range { start: Position { line: 0, character: 7 }, end: Position { line: 0, character: 9 } },
        );
        let checker = [diag];
        let lists = OracleDiagnosticLists {
            parse: &[],
            comment: &[],
            bind: &[],
            checker: &checker,
            task_list: &[],
            final_: &checker,
        };
        assert_eq!(
            dump_diagnostics(&lists),
            "parse: [0]\ncomment: [0]\nbind: [0]\nchecker: [1]\n  [0]: 3 0:7-0:9 - \"\\\"os\\\" is not accessed\"\n\
             taskList: [0]\nfinal: [1]\n  [0]: 3 0:7-0:9 - \"\\\"os\\\" is not accessed\"\n"
        );
    }
}
