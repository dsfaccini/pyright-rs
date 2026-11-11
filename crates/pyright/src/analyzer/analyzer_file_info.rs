//! Port of `packages/pyright-internal/src/analyzer/analyzerFileInfo.ts`.
//!

use std::cell::RefCell;
use std::rc::Rc;

use jsrt::{FxHashSet, JsMap, JsSet, P};

use crate::analyzer::scope::Scope;
use crate::analyzer::source_file::IPythonMode;
use crate::analyzer::symbol::SymbolTable;
use crate::common::config_options::{DefineConstantValue, DiagnosticRuleSet, ExecutionEnvironment};
use crate::common::diagnostic_sink::TextRangeDiagnosticSink;
use crate::common::python_version::{PYTHON_VERSION3_14, PythonVersion};
use crate::common::text_range::TextRange;
use crate::common::text_range_collection::TextRangeCollection;
use crate::common::uri::Uri;

// analyzer/analyzerFileInfo.ts:22
#[derive(Clone, Debug)]
#[derive(jsrt::Trace)]
pub struct AbsoluteModuleDescriptor {
    pub importing_file_uri: Uri,
    pub name_parts: Vec<String>,
}

/// The `Uri | AbsoluteModuleDescriptor` argument of `ImportLookup`.
#[derive(Clone, Debug)]
#[derive(jsrt::Trace)]
pub enum ImportLookupKey {
    Uri(Uri),
    Module(AbsoluteModuleDescriptor),
}

// analyzer/analyzerFileInfo.ts:27
#[derive(Clone, Copy, Debug, Default)]
#[derive(jsrt::Trace)]
pub struct LookupImportOptions {
    pub skip_file_needed_check: bool,
    pub skip_parsing: Option<bool>,
}

// analyzer/analyzerFileInfo.ts:37
#[derive(jsrt::Trace)]
pub struct ImportLookupResult {
    pub symbol_table: SymbolTable,
    pub dunder_all_names: Option<Vec<String>>,
    pub uses_unsupported_dunder_all_form: bool,
    /// A getter in TypeScript: computed when read.
    /// Captures a module node, a program handle.
    #[trace(skip)]
    pub doc_string: Rc<dyn Fn() -> Option<String>>,
    pub is_in_py_typed_package: bool,
}

// analyzer/analyzerFileInfo.ts:32
pub type ImportLookup = Rc<dyn Fn(ImportLookupKey, Option<LookupImportOptions>) -> Option<ImportLookupResult>>;

// analyzer/analyzerFileInfo.ts:45
#[derive(jsrt::Trace)]
pub struct AnalyzerFileInfo {
    /// Captures the `Program`, a program handle.
    #[trace(skip)]
    pub import_lookup: ImportLookup,
    pub future_imports: JsSet<String>,
    pub builtins_scope: Option<P<Scope>>,
    pub diagnostic_sink: TextRangeDiagnosticSink,
    pub execution_environment: P<ExecutionEnvironment>,
    pub diagnostic_rule_set: DiagnosticRuleSet,
    pub lines: &'static TextRangeCollection<TextRange>,
    pub typing_symbol_aliases: JsMap<String, String>,
    pub defined_constants: JsMap<String, DefineConstantValue>,
    pub file_id: String,
    pub file_uri: Uri,
    pub module_name: String,
    pub is_stub_file: bool,
    pub is_typing_stub_file: bool,
    pub is_typing_extensions_stub_file: bool,
    pub is_typeshed_stub_file: bool,
    pub is_built_in_stub_file: bool,
    pub is_in_py_typed_package: bool,
    pub ipython_mode: IPythonMode,
    /// A lookup-only set of symbol ids.
    pub accessed_symbol_set: RefCell<FxHashSet<u32>>,
}

// analyzer/analyzerFileInfo.ts:68
pub fn is_annotation_evaluation_postponed(file_info: &AnalyzerFileInfo) -> bool {
    if file_info.is_stub_file {
        return true;
    }

    if file_info.future_imports.contains("annotations") {
        return true;
    }

    PythonVersion::is_greater_or_equal_to(&file_info.execution_environment.python_version, &PYTHON_VERSION3_14)
}
