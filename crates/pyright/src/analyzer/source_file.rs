//! Port of `packages/pyright-internal/src/analyzer/sourceFile.ts`.
//!
//! Not ported: edit mode, open-file (client) contents beyond the fields the
//! CLI path reads, notebook cell chaining, the log tracker (it only logs at
//! `LogLevel.Log`, which the CLI never selects) and `test_*` helpers.
//! `parse_file_contents`, `run_binder` and `run_checker` are the single calls
//! into the parser, binder and checker; the first two are stand-ins until
//! those land.

use jsrt::cell::{Cell, RefCell};
use std::rc::Rc;

use jsrt::{JsMap, JsSet, JsStr, JsString, P, SwitchMode};

use crate::common::timing::{Duration, timing_stats};
use crate::analyzer::analyzer_file_info::{AnalyzerFileInfo, ImportLookup};
use crate::analyzer::analyzer_node_info::{
    AnalyzerNodeInfoContext, AnalyzerNodeInfoContextImpl, AnalyzerNodeInfoReader, create_analyzer_node_info_accessor,
    get_dunder_all_info,
};
use crate::analyzer::binder::Binder;
use crate::analyzer::checker::Checker;
use crate::analyzer::type_evaluator::TypeEvaluator;
use crate::analyzer::circular_dependency::CircularDependency;
use crate::analyzer::scope::Scope;
use crate::analyzer::symbol::SymbolTable;
use crate::analyzer::comment_utils::{CommentDiagnostic, get_file_level_directives};
use crate::analyzer::import_resolver::{ImportResolver, ImportedModuleDescriptor};
use crate::analyzer::import_result::ImportResult;
use crate::common::config_options::{ConfigOptions, DiagnosticLevel, DiagnosticRuleSet, ExecutionEnvironment, get_basic_diagnostic_rule_set};
use crate::common::console::ConsoleInterface;
use crate::common::debug::assert;
use crate::common::diagnostic::{Diagnostic, DiagnosticCategory, TaskListToken, convert_level_to_category};
use crate::common::diagnostic_rules::DiagnosticRule;
use crate::common::diagnostic_sink::{DiagnosticSink, TextRangeDiagnosticSink};
use crate::common::file_system::FileSystem;
use crate::common::path_utils::strip_file_extension;
use crate::common::position_utils::{convert_offsets_to_range, convert_text_range_to_range};
use crate::common::string_utils::{StringFingerprint, are_string_fingerprints_equal, get_string_fingerprint};
use crate::common::text_range::{Range, TextRange, get_empty_range};
use crate::common::text_range_collection::TextRangeCollection;
use crate::common::uri::Uri;
use crate::localization::localize::LocMessage;
use crate::oracle::dumper::{oracle_dump_bind, oracle_dump_parse};
use crate::parse_ahead;
use crate::parser::parse_nodes::{ModuleNode, ParseNode, with_node_id_block};
use crate::parser::parser::{ModuleImport, ParseFileResults, ParseOptions, Parser, ParserOutput};
use crate::parser::tokenizer::{IgnoreComment, TokenizerOutput};

// analyzer/sourceFile.ts:57
const MAX_IMPORT_CYCLES_PER_FILE: usize = 4;

// analyzer/sourceFile.ts:61
pub const MAX_SOURCE_FILE_SIZE: u64 = 50 * 1024 * 1024;

// analyzer/sourceFile.ts:63
struct ResolveImportResult {
    imports: Vec<P<ImportResult>>,
    builtins_import_result: Option<P<ImportResult>>,
    import_info: Vec<ImportInfoRecord>,
}

// analyzer/sourceFile.ts:69
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ImportInfoRecord {
    pub node: P<ParseNode>,
    pub import_result: P<ImportResult>,
}

// analyzer/sourceFile.ts:76
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(jsrt::Trace)]
pub enum IPythonMode {
    None = 0,
    CellDocs,
}

thread_local! {
    // analyzer/sourceFile.ts:85
    static NEXT_UNIQUE_FILE_ID: Cell<u32> = const { Cell::new(1) };
}

// analyzer/sourceFile.ts:87
#[derive(jsrt::Trace)]
pub struct WriteableData {
    pub diagnostic_version: u32,
    pub file_contents_version: u32,
    pub semantic_version: u32,
    pub last_file_content_length: Option<i32>,
    pub last_file_content_hash: Option<StringFingerprint>,
    pub client_document_contents: Option<String>,
    pub client_document_version: Option<i32>,
    pub analyzed_file_contents_version: i64,
    pub parsed_file_contents: Option<Rc<str>>,
    pub parsed_file_contents_hash: Option<StringFingerprint>,
    pub tokenizer_lines: Option<&'static TextRangeCollection<TextRange>>,
    pub tokenizer_output: Option<Rc<TokenizerOutput>>,
    pub line_count: Option<i32>,
    pub is_binding_in_progress: bool,
    pub is_checking_in_progress: bool,
    pub parse_diagnostics: Vec<P<Diagnostic>>,
    pub comment_diagnostics: Vec<P<Diagnostic>>,
    pub bind_diagnostics: Vec<P<Diagnostic>>,
    pub checker_diagnostics: Vec<P<Diagnostic>>,
    pub task_list_diagnostics: Vec<P<Diagnostic>>,
    pub type_ignore_lines: JsMap<i32, IgnoreComment>,
    pub type_ignore_all: Option<IgnoreComment>,
    pub pyright_ignore_lines: JsMap<i32, IgnoreComment>,
    pub accumulated_diagnostics: Vec<P<Diagnostic>>,
    pub diagnostics_without_file_ignore: Vec<P<Diagnostic>>,
    pub circular_dependencies: Vec<CircularDependency>,
    pub no_circular_dependency_confirmed: bool,
    pub hit_max_import_depth: Option<i32>,
    pub is_binding_needed: bool,
    pub is_checking_needed: bool,
    pub check_time: Option<f64>,
    pub imports: Option<Vec<P<ImportResult>>>,
    pub builtins_import: Option<P<ImportResult>>,
    pub import_info: Vec<ImportInfoRecord>,
    pub is_file_deleted: bool,
    pub parser_output: Option<P<ParserOutput>>,
    pub module_symbol_table: Option<SymbolTable>,
}

impl WriteableData {
    // analyzer/sourceFile.ts:165
    fn new() -> WriteableData {
        WriteableData {
            diagnostic_version: 0,
            file_contents_version: 0,
            semantic_version: 0,
            last_file_content_length: None,
            last_file_content_hash: None,
            client_document_contents: None,
            client_document_version: None,
            analyzed_file_contents_version: -1,
            parsed_file_contents: None,
            parsed_file_contents_hash: None,
            tokenizer_lines: None,
            tokenizer_output: None,
            line_count: None,
            is_binding_in_progress: false,
            is_checking_in_progress: false,
            parse_diagnostics: Vec::new(),
            comment_diagnostics: Vec::new(),
            bind_diagnostics: Vec::new(),
            checker_diagnostics: Vec::new(),
            task_list_diagnostics: Vec::new(),
            type_ignore_lines: JsMap::default(),
            type_ignore_all: None,
            pyright_ignore_lines: JsMap::default(),
            accumulated_diagnostics: Vec::new(),
            diagnostics_without_file_ignore: Vec::new(),
            circular_dependencies: Vec::new(),
            no_circular_dependency_confirmed: false,
            hit_max_import_depth: None,
            is_binding_needed: true,
            is_checking_needed: true,
            check_time: None,
            imports: None,
            builtins_import: None,
            import_info: Vec::new(),
            is_file_deleted: false,
            parser_output: None,
            module_symbol_table: None,
        }
    }
}

/// `(file: Uri) => string`, the module-name getter the program passes in.
pub type ModuleNameGetter = Rc<dyn Fn(Uri) -> String>;

// analyzer/sourceFile.ts:209
/// Shared: `P<SourceFile>`. State that TypeScript writes after construction
/// sits in `RefCell`s; no borrow is held across a call into another file.
#[derive(jsrt::Trace)]
pub struct SourceFile {
    console: &'static dyn ConsoleInterface,
    uri: Uri,
    file_id: String,
    /// Captures the `Program`, a program handle.
    #[trace(skip)]
    module_name_getter: ModuleNameGetter,
    cached_module_name: RefCell<Option<String>>,
    is_stub_file: bool,
    is_third_party_import: bool,
    is_typing_stub_file: bool,
    is_typing_extensions_stub_file: bool,
    is_typeshed_stub_file: bool,
    is_built_in_stub_file: bool,
    is_third_party_py_typed_present: bool,
    diagnostic_rule_set: RefCell<DiagnosticRuleSet>,
    ipython_mode: IPythonMode,
    writable_data: RefCell<WriteableData>,
    pub file_system: &'static dyn FileSystem,
}

impl SourceFile {
    // analyzer/sourceFile.ts:272
    pub fn new(
        file_system: &'static dyn FileSystem,
        uri: Uri,
        module_name_getter: ModuleNameGetter,
        is_third_party_import: bool,
        is_third_party_py_typed_present: bool,
        console: &'static dyn ConsoleInterface,
        ipython_mode: Option<IPythonMode>,
    ) -> P<SourceFile> {
        let file_id = SourceFile::make_file_id(uri);
        let is_stub_file = uri.has_extension(".pyi");
        let file_name = uri.file_name();
        let is_typing_stub_file =
            is_stub_file && (uri.path_ends_with("stdlib/typing.pyi") || file_name == "typing_extensions.pyi");
        let is_typing_extensions_stub_file = is_stub_file && file_name == "typing_extensions.pyi";
        let is_typeshed_stub_file = is_stub_file
            && (uri.path_ends_with("stdlib/_typeshed/__init__.pyi")
                || uri.path_ends_with("stdlib/_typeshed/_type_checker_internals.pyi"));

        let mut is_built_in_stub_file = false;
        if is_stub_file
            && [
                "stdlib/collections/__init__.pyi",
                "stdlib/asyncio/futures.pyi",
                "stdlib/asyncio/tasks.pyi",
                "stdlib/builtins.pyi",
                "stdlib/_importlib_modulespec.pyi",
                "stdlib/dataclasses.pyi",
                "stdlib/abc.pyi",
                "stdlib/enum.pyi",
                "stdlib/queue.pyi",
                "stdlib/string/templatelib.pyi",
                "stdlib/types.pyi",
                "stdlib/warnings.pyi",
            ]
            .iter()
            .any(|suffix| uri.path_ends_with(suffix))
        {
            is_built_in_stub_file = true;
        }

        P::new(SourceFile {
            console,
            uri,
            file_id,
            module_name_getter,
            cached_module_name: RefCell::new(None),
            is_stub_file,
            is_third_party_import,
            is_typing_stub_file,
            is_typing_extensions_stub_file,
            is_typeshed_stub_file,
            is_built_in_stub_file,
            is_third_party_py_typed_present,
            diagnostic_rule_set: RefCell::new(get_basic_diagnostic_rule_set()),
            ipython_mode: ipython_mode.unwrap_or(IPythonMode::None),
            writable_data: RefCell::new(WriteableData::new()),
            file_system,
        })
    }

    /// Read access to the writable data, for dumpers and the program.
    pub fn data(&self) -> jsrt::cell::Ref<'_, WriteableData> {
        self.writable_data.borrow()
    }

    fn data_mut(&self) -> jsrt::cell::RefMut<'_, WriteableData> {
        self.writable_data.borrow_mut()
    }

    // analyzer/sourceFile.ts:331
    pub fn set_initial_diagnostic_rule_set(&self, rule_set: &DiagnosticRuleSet) {
        *self.diagnostic_rule_set.borrow_mut() = rule_set.clone();
    }

    // analyzer/sourceFile.ts:335
    pub fn get_ipython_mode(&self) -> IPythonMode {
        self.ipython_mode
    }

    // analyzer/sourceFile.ts:339
    pub fn get_uri(&self) -> Uri {
        self.uri
    }

    // analyzer/sourceFile.ts:343
    /// Pyright never stores the getter's result, so `_cachedModuleName` stays unset.
    pub fn get_module_name(&self) -> String {
        if let Some(name) = self.cached_module_name.borrow().as_ref().filter(|n| !n.is_empty()) {
            return name.clone();
        }

        let name = (self.module_name_getter)(self.uri);
        if !name.is_empty() { name } else { strip_file_extension(self.uri.file_name(), false) }
    }

    // analyzer/sourceFile.ts:353
    pub fn clear_cached_module_name(&self) {
        *self.cached_module_name.borrow_mut() = None;
    }

    // analyzer/sourceFile.ts:357
    pub fn get_diagnostic_version(&self) -> u32 {
        self.data().diagnostic_version
    }

    // analyzer/sourceFile.ts:361
    pub fn get_parse_diagnostics(&self) -> Vec<P<Diagnostic>> {
        self.data().parse_diagnostics.clone()
    }

    // analyzer/sourceFile.ts:365
    pub fn is_stub_file(&self) -> bool {
        self.is_stub_file
    }

    // analyzer/sourceFile.ts:369
    pub fn is_typing_stub_file(&self) -> bool {
        self.is_typing_stub_file
    }

    // analyzer/sourceFile.ts:373
    pub fn is_typeshed_stub_file(&self) -> bool {
        self.is_typeshed_stub_file
    }

    // analyzer/sourceFile.ts:377
    pub fn is_built_in_stub_file(&self) -> bool {
        self.is_built_in_stub_file
    }

    // analyzer/sourceFile.ts:381
    pub fn is_third_party_py_typed_present(&self) -> bool {
        self.is_third_party_py_typed_present
    }

    // analyzer/sourceFile.ts:388
    pub fn get_diagnostics(&self, _options: &ConfigOptions, prev_diagnostic_version: Option<u32>) -> Option<Vec<P<Diagnostic>>> {
        let data = self.data();
        if Some(data.diagnostic_version) == prev_diagnostic_version {
            return None;
        }

        Some(data.accumulated_diagnostics.clone())
    }

    // analyzer/sourceFile.ts:400
    pub fn get_imports(&self) -> Vec<P<ImportResult>> {
        self.data().imports.clone().unwrap_or_default()
    }

    // analyzer/sourceFile.ts:404
    pub fn get_builtins_import(&self) -> Option<P<ImportResult>> {
        self.data().builtins_import
    }

    // analyzer/sourceFile.ts:431
    pub fn did_contents_change_on_disk(&self) -> bool {
        let (client_contents, last_length, last_hash) = {
            let data = self.data();
            (data.client_document_contents.clone(), data.last_file_content_length, data.last_file_content_hash)
        };
        if client_contents.is_some_and(|c| !c.is_empty()) {
            return false;
        }

        let Some(last_length) = last_length else {
            return true;
        };

        if !self.file_system.exists_sync(self.uri) {
            return true;
        }
        let Ok(file_contents) = self.file_system.read_file_text_sync(self.uri) else {
            return true;
        };
        if file_contents.js_len() != last_length {
            return true;
        }

        let content_hash = get_string_fingerprint(&file_contents);
        match last_hash {
            Some(last_hash) if are_string_fingerprints_equal(&content_hash, &last_hash) => false,
            _ => true,
        }
    }

    // analyzer/sourceFile.ts:477
    pub fn drop_parse_and_bind_info(&self) -> Option<ModuleNode> {
        let mut data = self.data_mut();
        if data.is_binding_in_progress || data.is_checking_in_progress {
            return None;
        }

        let parse_tree = data.parser_output.map(|o| o.parse_tree);
        data.parser_output = None;
        data.tokenizer_lines = None;
        data.tokenizer_output = None;
        data.parsed_file_contents = None;
        data.parsed_file_contents_hash = None;
        data.module_symbol_table = None;
        data.is_binding_needed = true;
        data.import_info = Vec::new();
        data.imports = Some(Vec::new());
        parse_tree
    }

    // analyzer/sourceFile.ts:499
    pub fn mark_dirty(&self) {
        let mut data = self.data_mut();
        data.file_contents_version += 1;
        data.semantic_version += 1;
        data.no_circular_dependency_confirmed = false;
        data.is_checking_needed = true;
        data.is_binding_needed = true;
        data.module_symbol_table = None;
        data.line_count = None;
    }

    // analyzer/sourceFile.ts:511
    pub fn mark_reanalysis_required(&self, force_rebinding: bool, node_info_reader: &dyn AnalyzerNodeInfoReader) {
        let mut data = self.data_mut();
        data.semantic_version += 1;
        data.is_checking_needed = true;
        data.no_circular_dependency_confirmed = false;

        // If the file contains a wildcard import or __all__ symbols,
        // we need to rebind because a dependent import may have changed.
        if let Some(parser_output) = data.parser_output {
            if parser_output.contains_wildcard_import
                || get_dunder_all_info(parser_output.parse_tree, node_info_reader).is_some()
                || force_rebinding
            {
                data.is_binding_needed = true;
                data.module_symbol_table = None;
            }
        }
    }

    // analyzer/sourceFile.ts:408
    pub fn get_module_symbol_table(&self) -> Option<SymbolTable> {
        self.data().module_symbol_table
    }

    // analyzer/sourceFile.ts:539
    pub fn get_client_version(&self) -> Option<i32> {
        self.data().client_document_version
    }

    // analyzer/sourceFile.ts:551
    pub fn get_open_file_contents(&self) -> Option<String> {
        self.data().client_document_contents.clone()
    }

    // analyzer/sourceFile.ts:555
    pub fn get_file_content(&self) -> Option<String> {
        if let Some(open_file_content) = self.get_open_file_contents() {
            return Some(open_file_content);
        }

        if !self.is_parse_required() {
            if let Some(parsed) = self.data().parsed_file_contents.clone() {
                return Some(parsed.to_string());
            }
        }

        let file_stat = self.file_system.stat_sync(self.uri).ok()?;
        if file_stat.size > MAX_SOURCE_FILE_SIZE {
            self.console.error(&format!(
                "File length of \"{}\" is {} which exceeds the maximum supported file size of {MAX_SOURCE_FILE_SIZE}",
                self.uri.to_uri_string(),
                file_stat.size
            ));
            return None;
        }

        self.read_file_contents()
    }

    // analyzer/sourceFile.ts:617
    /// Fires the file-dirty event, which has no listeners on the CLI path.
    pub fn prepare_for_close(&self) {}

    // analyzer/sourceFile.ts:621
    pub fn is_file_deleted(&self) -> bool {
        self.data().is_file_deleted
    }

    // analyzer/sourceFile.ts:625
    pub fn is_parse_required(&self) -> bool {
        let data = self.data();
        data.parser_output.is_none() || data.analyzed_file_contents_version != data.file_contents_version as i64
    }

    // analyzer/sourceFile.ts:632
    pub fn is_binding_required(&self) -> bool {
        if self.data().is_binding_in_progress {
            return false;
        }

        if self.is_parse_required() {
            return true;
        }

        self.data().is_binding_needed
    }

    // analyzer/sourceFile.ts:644
    pub fn is_checking_required(&self) -> bool {
        self.data().is_checking_needed
    }

    // analyzer/sourceFile.ts:678
    pub fn get_parser_output(&self) -> Option<P<ParserOutput>> {
        if self.is_parse_required() {
            return None;
        }

        let output = self.data().parser_output;
        assert(output.is_some(), None);
        output
    }

    // analyzer/sourceFile.ts:690
    pub fn add_circular_dependency(&self, config_options: &ConfigOptions, circ_dependency: CircularDependency) {
        let mut updated_dependency_list = false;

        {
            let mut data = self.data_mut();
            if data.circular_dependencies.len() < MAX_IMPORT_CYCLES_PER_FILE
                && !data.circular_dependencies.iter().any(|dep| dep.is_equal(&circ_dependency))
            {
                data.circular_dependencies.push(circ_dependency);
                updated_dependency_list = true;
            }
        }

        if updated_dependency_list {
            self.recompute_diagnostics(config_options);
        }
    }

    // analyzer/sourceFile.ts:706
    pub fn set_no_circular_dependency_confirmed(&self) {
        self.data_mut().no_circular_dependency_confirmed = true;
    }

    // analyzer/sourceFile.ts:710
    pub fn is_no_circular_dependency_confirmed(&self) -> bool {
        !self.is_parse_required() && self.data().no_circular_dependency_confirmed
    }

    // analyzer/sourceFile.ts:714
    pub fn set_hit_max_import_depth(&self, max_import_depth: i32) {
        self.data_mut().hit_max_import_depth = Some(max_import_depth);
    }

    // analyzer/sourceFile.ts:721
    pub fn parse(&self, config_options: &ConfigOptions, import_resolver: &ImportResolver, content: Option<&str>) -> bool {
        if !self.is_parse_required() {
            return false;
        }

        // Rust-only: a parse worker may have read and parsed the file already
        // (`docs/porting.md`, "parse ahead").
        let ahead = if content.is_none() && parse_ahead::active() {
            let now = self.parse_ahead_request(config_options);
            timing_stats().parse_file_time.time_operation(|| parse_ahead::take(self.uri.key(), now.as_ref()))
        } else {
            None
        };
        let (diag_sink, file_contents, parse_file_results) = match ahead {
            Some(parsed) => {
                if parse_ahead::mode() == SwitchMode::Check {
                    let (diag_sink, contents, results) = self.read_and_parse(config_options, content);
                    let data = self.data();
                    let (content_length, content_hash) =
                        (data.last_file_content_length.unwrap(), data.last_file_content_hash.unwrap());
                    drop(data);
                    let here = parse_ahead::Parsed { contents, content_hash, content_length, diag_sink, results };
                    parse_ahead::check(self.uri.get_file_path(), &parsed, &here);
                }
                let mut data = self.data_mut();
                data.last_file_content_length = Some(parsed.content_length);
                data.last_file_content_hash = Some(parsed.content_hash);
                drop(data);
                (parsed.diag_sink, parsed.contents, parsed.results)
            }
            None => self.read_and_parse(config_options, content),
        };

        oracle_dump_parse(self.uri, &parse_file_results);
        let ParseFileResults { text, content_hash: parsed_content_hash, parser_output, tokenizer_output } =
            parse_file_results;
        let parser_output = P::new(parser_output);
        // Dropped at the end of this function unless the file is open, as in
        // pyright: the tokens no parse node holds are freed with it.
        let tokenizer_output = Rc::new(tokenizer_output);
        {
            let mut data = self.data_mut();
            data.parser_output = Some(parser_output);
            data.tokenizer_lines = Some(tokenizer_output.lines.get());
            data.parsed_file_contents = Some(Rc::from(file_contents.as_str()));
            data.parsed_file_contents_hash = Some(parsed_content_hash);
            data.type_ignore_lines = tokenizer_output.type_ignore_lines.clone();
            data.type_ignore_all = tokenizer_output.type_ignore_all.clone();
            data.pyright_ignore_lines = tokenizer_output.pyright_ignore_lines.clone();
            data.line_count = Some(tokenizer_output.lines.count());

            // Cache the tokenizer output only if this file is open.
            if data.client_document_contents.is_some() {
                data.tokenizer_output = Some(tokenizer_output.clone());
            }
        }

        // Resolve imports.
        let exec_environment = config_options.find_exec_environment(self.uri);
        timing_stats().resolve_imports_time.time_operation(|| {
            let import_result = self.resolve_imports(import_resolver, &parser_output.imported_modules, exec_environment);

            {
                let mut data = self.data_mut();
                data.imports = Some(import_result.imports);
                data.builtins_import = import_result.builtins_import_result;
                data.import_info = import_result.import_info;

                data.parse_diagnostics = diag_sink.fetch_and_clear();
            }

            let mut task_list_diagnostics = Vec::new();
            add_task_list_diagnostics(
                config_options.task_list_tokens.as_deref(),
                &text,
                &tokenizer_output,
                &mut task_list_diagnostics,
            );
            self.data_mut().task_list_diagnostics = task_list_diagnostics;
        });

        // Is this file in a "strict" path?
        let use_strict = config_options.strict.iter().any(|strict_file_spec| self.uri.matches_regex(&strict_file_spec.reg_exp));

        let mut comment_diags: Vec<CommentDiagnostic> = Vec::new();
        let rule_set = get_file_level_directives(
            &tokenizer_output.tokens,
            &tokenizer_output.lines,
            &exec_environment.diagnostic_rule_set,
            use_strict,
            &mut comment_diags,
        );
        *self.diagnostic_rule_set.borrow_mut() = rule_set;

        let comment_diagnostics: Vec<P<Diagnostic>> = comment_diags
            .into_iter()
            .map(|comment_diag| {
                Diagnostic::new(
                    DiagnosticCategory::Error,
                    comment_diag.message,
                    convert_text_range_to_range(&comment_diag.range, &tokenizer_output.lines),
                )
            })
            .collect();

        {
            let mut data = self.data_mut();
            data.comment_diagnostics = comment_diagnostics;
            data.analyzed_file_contents_version = data.file_contents_version as i64;
            data.is_binding_needed = true;
            data.is_checking_needed = true;
            data.hit_max_import_depth = None;
        }

        self.recompute_diagnostics(config_options);

        true
    }

    /// The part of `parse` that reads and parses the file, on this thread.
    /// A check with parse workers numbers the file's nodes in one block.
    // analyzer/sourceFile.ts:729
    fn read_and_parse(
        &self,
        config_options: &ConfigOptions,
        content: Option<&str>,
    ) -> (P<DiagnosticSink>, String, ParseFileResults) {
        let diag_sink = P::new(DiagnosticSink::new(None));
        let mut file_contents = self.get_open_file_contents();
        let mut content_hash = if file_contents.is_some() { self.data().last_file_content_hash } else { None };
        if file_contents.is_none() {
            let read = timing_stats().read_file_time.time_operation(|| {
                // Read the file's contents.
                let contents = content.map(str::to_string).or_else(|| self.get_file_content())?;

                // Remember the length and fingerprint for comparison purposes.
                let hash = get_string_fingerprint(&contents);
                let mut data = self.data_mut();
                data.last_file_content_length = Some(contents.js_len());
                data.last_file_content_hash = Some(hash);
                Some((contents, hash))
            });
            match read {
                Some((contents, hash)) => {
                    content_hash = Some(hash);
                    file_contents = Some(contents);
                }
                None => {
                    diag_sink.add_error("Source file could not be read".to_string(), get_empty_range());
                    file_contents = Some(String::new());
                    content_hash = Some(get_string_fingerprint(""));

                    if !self.file_system.exists_sync(self.uri) {
                        self.data_mut().is_file_deleted = true;
                    }
                }
            }
        }

        let file_contents = file_contents.unwrap();
        let content_hash = content_hash.unwrap_or_else(|| get_string_fingerprint(&file_contents));

        // Parse the token stream, building the abstract syntax tree.
        let parse = || {
            self.parse_file(
                config_options,
                self.uri,
                &file_contents,
                self.ipython_mode != IPythonMode::None,
                diag_sink,
                content_hash,
            )
        };
        let parse_file_results = if parse_ahead::active() { with_node_id_block(parse) } else { parse() };

        (diag_sink, file_contents, parse_file_results)
    }

    /// Rust-only: how a parse worker reads and parses this file, when one can
    /// (`docs/porting.md`, "parse ahead"): not for an open file, nor for a file
    /// the file system maps to no single real file.
    pub fn parse_ahead_request(&self, config_options: &ConfigOptions) -> Option<parse_ahead::Request> {
        if !self.is_parse_required() || self.data().client_document_contents.is_some() {
            return None;
        }
        let path = self.file_system.real_file_path(self.uri)?;
        Some(parse_ahead::Request { path, options: self.parse_options(config_options) })
    }

    // analyzer/sourceFile.ts:905
    /// Returns whether the file was bound. The CLI has no cell chain index.
    pub fn bind(
        &self,
        config_options: &ConfigOptions,
        import_lookup: ImportLookup,
        builtins_scope: Option<P<Scope>>,
        future_imports: JsSet<String>,
        node_info_context: P<AnalyzerNodeInfoContextImpl>,
    ) -> bool {
        assert(!self.is_parse_required(), Some("Bind called before parsing"));
        assert(self.is_binding_required(), Some("Bind called unnecessarily"));
        assert(!self.data().is_binding_in_progress, Some("Bind called while binding in progress"));
        let parser_output = self.data().parser_output;
        assert(parser_output.is_some(), Some("Parse results not available"));
        let parser_output = parser_output.unwrap();

        // Perform name binding.
        timing_stats().bind_time.time_operation(|| {
            let parse_tree = parser_output.parse_tree;
            let binding_session = node_info_context.begin_write(parse_tree);
            let node_info = create_analyzer_node_info_accessor(binding_session.get(), Some(binding_session.get()));
            let import_info = self.data().import_info.clone();
            for record in &import_info {
                node_info.set_import_info(record.node, record.import_result);
            }

            let file_info = P::new(self.build_file_info(config_options, import_lookup, builtins_scope, future_imports));
            node_info.set_file_info(parse_tree, file_info);

            let mut binder = Binder::new(file_info, config_options.index_generation_mode, None, node_info);
            self.data_mut().is_binding_in_progress = true;
            binder.bind_module(parse_tree);

            let bind_diagnostics = file_info.diagnostic_sink.fetch_and_clear();
            let module_scope = node_info.get_scope(parse_tree);
            assert(module_scope.is_some(), Some("Module scope not returned by binder"));

            node_info_context.publish(binding_session);
            oracle_dump_bind(self.uri, parse_tree, node_info_context);
            let mut data = self.data_mut();
            data.module_symbol_table = Some(module_scope.unwrap().symbol_table);
            data.bind_diagnostics = bind_diagnostics;
        });
        {
            let mut data = self.data_mut();
            data.is_binding_in_progress = false;

            // Prepare for the next stage of the analysis.
            data.is_checking_needed = true;
            data.is_binding_needed = false;
        }

        self.recompute_diagnostics(config_options);
        true
    }

    // analyzer/sourceFile.ts:990
    /// Runs the checker through `run_checker`.
    pub fn check(
        &self,
        config_options: &ConfigOptions,
        import_lookup: ImportLookup,
        import_resolver: P<ImportResolver>,
        evaluator: P<TypeEvaluator>,
        dependent_files: Option<Vec<ParserOutput>>,
        node_info_reader: P<dyn AnalyzerNodeInfoReader>,
    ) {
        assert(!self.is_parse_required(), Some("Check called before parsing"));
        assert(!self.is_binding_required(), Some("Check called before binding"));
        assert(!self.data().is_binding_in_progress, Some("Check called while binding in progress"));
        assert(!self.data().is_checking_in_progress, Some("Check called while checking in progress"));
        assert(self.is_checking_required(), Some("Check called unnecessarily"));
        assert(self.data().parser_output.is_some(), Some("Parse results not available"));
        let _ = import_lookup;

        timing_stats().type_checker_time.time_operation(|| {
            let check_duration = Duration::new();
            self.data_mut().is_checking_in_progress = true;
            let checker_diagnostics = run_checker(self, import_resolver, evaluator, dependent_files, node_info_reader);
            let mut data = self.data_mut();
            data.is_checking_needed = false;
            data.checker_diagnostics = checker_diagnostics;
            data.check_time = Some(check_duration.get_duration_in_milliseconds());
        });
        {
            let mut data = self.data_mut();
            data.is_checking_in_progress = false;

            // Clear any circular dependencies associated with this file.
            data.circular_dependencies = Vec::new();
        }

        self.recompute_diagnostics(config_options);
    }

    // analyzer/sourceFile.ts:412
    pub fn get_check_time(&self) -> Option<f64> {
        self.data().check_time
    }

    // analyzer/sourceFile.ts:1096
    fn make_file_id(uri: Uri) -> String {
        let max_name_length = 8;

        let mut file_name = uri.file_name_without_extensions().to_string();
        if file_name.js_len() > max_name_length {
            file_name = file_name.js_substring(file_name.js_len() - max_name_length, None).into_owned();
        }

        let unique_number = NEXT_UNIQUE_FILE_ID.with(|n| {
            let v = n.get();
            n.set(v + 1);
            v
        });

        format!("{file_name}/{unique_number}")
    }

    // analyzer/sourceFile.ts:1115
    fn recompute_diagnostics(&self, config_options: &ConfigOptions) {
        let rule_set = self.diagnostic_rule_set.borrow().clone();
        let mut data = self.data_mut();
        data.diagnostic_version += 1;

        let mut include_warnings_and_errors = true;

        // If a file was imported as a third-party file, don't report
        // any errors for it. The user can't fix them anyway.
        if self.is_third_party_import {
            include_warnings_and_errors = false;
        }

        let mut diag_list: Vec<P<Diagnostic>> = Vec::new();
        diag_list.extend_from_slice(&data.parse_diagnostics);
        diag_list.extend_from_slice(&data.comment_diagnostics);
        diag_list.extend_from_slice(&data.bind_diagnostics);
        diag_list.extend_from_slice(&data.checker_diagnostics);
        diag_list.extend_from_slice(&data.task_list_diagnostics);

        let prefiltered_diag_list = diag_list.clone();
        let mut type_ignore_lines_clone = data.type_ignore_lines.clone();
        let mut pyright_ignore_lines_clone = data.pyright_ignore_lines.clone();

        let is_hint = |d: &Diagnostic| {
            matches!(
                d.category,
                DiagnosticCategory::UnusedCode | DiagnosticCategory::UnreachableCode | DiagnosticCategory::Deprecated
            )
        };

        // Filter the diagnostics based on "type: ignore" lines.
        if rule_set.enable_type_ignore_comments && !data.type_ignore_lines.is_empty() {
            diag_list.retain(|d| {
                if !is_hint(d) {
                    for line in d.range.start.line..=d.range.end.line {
                        if data.type_ignore_lines.contains_key(&line) {
                            type_ignore_lines_clone.shift_remove(&line);
                            return false;
                        }
                    }
                }

                true
            });
        }

        // Filter the diagnostics based on "pyright: ignore" lines.
        if !data.pyright_ignore_lines.is_empty() {
            diag_list.retain(|d| {
                if !is_hint(d) {
                    for line in d.range.start.line..=d.range.end.line {
                        if let Some(pyright_ignore_comment) = data.pyright_ignore_lines.get(&line) {
                            let Some(rules_list) = &pyright_ignore_comment.rules_list else {
                                pyright_ignore_lines_clone.shift_remove(&line);
                                return false;
                            };

                            let Some(diag_rule) = d.get_rule() else {
                                // If there's no diagnostic rule, it won't match
                                // against a rules list.
                                return true;
                            };

                            // Did we find this rule in the list?
                            if rules_list.iter().any(|rule| rule.text == diag_rule) {
                                // Update the pyrightIgnoreLinesClone to remove this rule.
                                if let Some(old_clone) = pyright_ignore_lines_clone.get(&line).cloned() {
                                    if let Some(old_rules) = &old_clone.rules_list {
                                        let filtered_rules_list: Vec<_> =
                                            old_rules.iter().filter(|rule| rule.text != diag_rule).cloned().collect();
                                        if filtered_rules_list.is_empty() {
                                            pyright_ignore_lines_clone.shift_remove(&line);
                                        } else {
                                            pyright_ignore_lines_clone.insert(
                                                line,
                                                IgnoreComment { range: old_clone.range, rules_list: Some(filtered_rules_list) },
                                            );
                                        }
                                    }
                                }

                                return false;
                            }

                            return true;
                        }
                    }
                }

                true
            });
        }

        let mut unnecessary_type_ignore_diags: Vec<P<Diagnostic>> = Vec::new();

        // Skip this step if type checking is needed. Otherwise we'll likely produce
        // incorrect (false positive) reportUnnecessaryTypeIgnoreComment diagnostics
        // until checking is performed on this file.
        if rule_set.report_unnecessary_type_ignore_comment != DiagnosticLevel::None && !data.is_checking_needed {
            let diag_category = convert_level_to_category(rule_set.report_unnecessary_type_ignore_comment);

            let prefiltered_error_list: Vec<&P<Diagnostic>> = prefiltered_diag_list
                .iter()
                .filter(|diag| {
                    matches!(
                        diag.category,
                        DiagnosticCategory::Error | DiagnosticCategory::Warning | DiagnosticCategory::Information
                    )
                })
                .collect();

            let is_unreachable_code_range = |range: &Range| {
                prefiltered_diag_list.iter().any(|diag| {
                    diag.category == DiagnosticCategory::UnreachableCode
                        && diag.range.start.line <= range.start.line
                        && diag.range.end.line >= range.end.line
                })
            };

            let lines = data.tokenizer_lines;
            let make_diag = |message: String, range: Range| {
                let diag = Diagnostic::new(diag_category, message, range);
                diag.set_rule(DiagnosticRule::reportUnnecessaryTypeIgnoreComment.as_str());
                diag
            };

            if prefiltered_error_list.is_empty() {
                if let Some(type_ignore_all) = &data.type_ignore_all {
                    let range_start = type_ignore_all.range.start;
                    let range_end = range_start + type_ignore_all.range.length;
                    let range = convert_offsets_to_range(range_start, range_end, lines.unwrap());

                    if !is_unreachable_code_range(&range) && rule_set.enable_type_ignore_comments {
                        unnecessary_type_ignore_diags.push(make_diag(LocMessage::unnecessary_type_ignore(), range));
                    }
                }
            }

            for ignore_comment in type_ignore_lines_clone.values() {
                if let Some(lines) = lines {
                    let range_start = ignore_comment.range.start;
                    let range_end = range_start + ignore_comment.range.length;
                    let range = convert_offsets_to_range(range_start, range_end, lines);

                    if !is_unreachable_code_range(&range) && rule_set.enable_type_ignore_comments {
                        unnecessary_type_ignore_diags.push(make_diag(LocMessage::unnecessary_type_ignore(), range));
                    }
                }
            }

            for ignore_comment in pyright_ignore_lines_clone.values() {
                if let Some(lines) = lines {
                    match &ignore_comment.rules_list {
                        None => {
                            let range_start = ignore_comment.range.start;
                            let range_end = range_start + ignore_comment.range.length;
                            let range = convert_offsets_to_range(range_start, range_end, lines);

                            if !is_unreachable_code_range(&range) {
                                unnecessary_type_ignore_diags.push(make_diag(LocMessage::unnecessary_type_ignore(), range));
                            }
                        }
                        Some(rules_list) => {
                            for unused_rule in rules_list {
                                let range_start = unused_rule.range.start;
                                let range_end = range_start + unused_rule.range.length;
                                let range = convert_offsets_to_range(range_start, range_end, lines);

                                if !is_unreachable_code_range(&range) {
                                    unnecessary_type_ignore_diags.push(make_diag(
                                        LocMessage::unnecessary_pyright_ignore_rule().format(&[("name", &unused_rule.text)]),
                                        range,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        if rule_set.report_import_cycles != DiagnosticLevel::None && !data.circular_dependencies.is_empty() {
            let category = convert_level_to_category(rule_set.report_import_cycles);

            for cir_dep in &data.circular_dependencies {
                let paths: Vec<String> = cir_dep.get_paths().iter().map(|path| format!("  {}", path.to_user_visible_string())).collect();
                let diag = Diagnostic::new(
                    category,
                    format!("{}\n{}", LocMessage::import_cycle_detected(), paths.join("\n")),
                    get_empty_range(),
                );
                diag.set_rule(DiagnosticRule::reportImportCycles.as_str());
                diag_list.push(diag);
            }
        }

        if let Some(depth) = data.hit_max_import_depth {
            diag_list.push(Diagnostic::new(
                DiagnosticCategory::Error,
                LocMessage::import_depth_exceeded().format(&[("depth", &depth.to_string())]),
                get_empty_range(),
            ));
        }

        // If there is a "type: ignore" comment at the top of the file, clear
        // the diagnostic list of all error, warning, and information diagnostics.
        if rule_set.enable_type_ignore_comments && data.type_ignore_all.is_some() {
            diag_list.retain(|diag| {
                !matches!(
                    diag.category,
                    DiagnosticCategory::Error | DiagnosticCategory::Warning | DiagnosticCategory::Information
                )
            });
        }

        // Now add in the "unnecessary type ignore" diagnostics.
        diag_list.extend(unnecessary_type_ignore_diags);

        // If we're not returning any diagnostics, filter out all of
        // the errors and warnings, leaving only the unreachable code
        // and deprecated diagnostics.
        if !include_warnings_and_errors {
            diag_list.retain(|diag| is_hint(diag));
        }

        data.diagnostics_without_file_ignore = diag_list.clone();

        // If the file is in the ignore list, clear the diagnostic list.
        if config_options.ignore.iter().any(|ignore_file_spec| self.uri.matches_regex(&ignore_file_spec.reg_exp)) {
            diag_list = Vec::new();
        }

        data.accumulated_diagnostics = diag_list;
    }

    // analyzer/sourceFile.ts:1478
    fn build_file_info(
        &self,
        config_options: &ConfigOptions,
        import_lookup: ImportLookup,
        builtins_scope: Option<P<Scope>>,
        future_imports: JsSet<String>,
    ) -> AnalyzerFileInfo {
        let (lines, parser_output) = {
            let data = self.data();
            (data.tokenizer_lines.unwrap(), data.parser_output.unwrap())
        };
        let analysis_diagnostics = TextRangeDiagnosticSink::new(lines, None);

        AnalyzerFileInfo {
            import_lookup,
            future_imports,
            builtins_scope,
            diagnostic_sink: analysis_diagnostics,
            execution_environment: config_options.find_exec_environment(self.uri),
            diagnostic_rule_set: self.diagnostic_rule_set.borrow().clone(),
            lines,
            typing_symbol_aliases: parser_output.typing_symbol_aliases.clone(),
            defined_constants: config_options.define_constant.clone(),
            file_id: self.file_id.clone(),
            file_uri: self.uri,
            module_name: Rc::new(self.get_module_name()),
            is_stub_file: self.is_stub_file,
            is_typing_stub_file: self.is_typing_stub_file,
            is_typing_extensions_stub_file: self.is_typing_extensions_stub_file,
            is_typeshed_stub_file: self.is_typeshed_stub_file,
            is_built_in_stub_file: self.is_built_in_stub_file,
            is_in_py_typed_package: self.is_third_party_py_typed_present,
            ipython_mode: self.ipython_mode,
            accessed_symbol_set: RefCell::new(Default::default()),
        }
    }

    // analyzer/sourceFile.ts:1512
    fn resolve_imports(
        &self,
        import_resolver: &ImportResolver,
        module_imports: &[ModuleImport],
        exec_env: P<ExecutionEnvironment>,
    ) -> ResolveImportResult {
        let mut imports: Vec<P<ImportResult>> = Vec::new();
        let mut import_info: Vec<ImportInfoRecord> = Vec::new();

        let resolve_and_add_if_not_self = |imports: &mut Vec<P<ImportResult>>, name_parts: &[&str], skip_missing_import: bool| {
            let import_result = import_resolver.resolve_import(
                self.uri,
                &exec_env,
                &ImportedModuleDescriptor {
                    leading_dots: 0,
                    name_parts: name_parts.iter().map(|s| s.to_string()).collect(),
                    has_trailing_dot: None,
                    imported_symbols: None,
                },
            );

            if skip_missing_import && !import_result.is_import_found {
                return None;
            }

            // Avoid importing module from the module file itself.
            if import_result.resolved_uris.is_empty() || !Uri::ptr_eq(import_result.resolved_uris[0], self.uri) {
                imports.push(import_result);
                return Some(import_result);
            }

            None
        };

        // Always include an implicit import of the builtins module.
        let mut builtins_import_result: Option<P<ImportResult>> = None;

        // If this is a project source file (not a stub), try to resolve
        // the __builtins__ stub first.
        if !self.is_third_party_import && !self.is_stub_file {
            builtins_import_result = resolve_and_add_if_not_self(&mut imports, &["__builtins__"], true);
        }

        if builtins_import_result.is_none() {
            builtins_import_result = resolve_and_add_if_not_self(&mut imports, &["builtins"], false);
        }

        resolve_and_add_if_not_self(&mut imports, &["_typeshed", "_type_checker_internals"], true);

        for module_import in module_imports {
            let import_result = import_resolver.resolve_import(
                self.uri,
                &exec_env,
                &ImportedModuleDescriptor {
                    leading_dots: module_import.leading_dots,
                    name_parts: module_import.name_parts.clone(),
                    has_trailing_dot: None,
                    imported_symbols: module_import.imported_symbols.clone(),
                },
            );

            imports.push(import_result);

            // Associate the import results with the module import
            // name node in the parse tree so we can access it later
            // (for hover and definition support).
            let name_node_parts = module_import.name_node.d().name_parts.borrow();
            if module_import.name_parts.len() == name_node_parts.len() {
                import_info.push(ImportInfoRecord { node: module_import.name_node.node(), import_result });
            } else {
                // For implicit imports of higher-level modules within a multi-part
                // module name, the moduleImport.nameParts will refer to the subset
                // of the multi-part name rather than the full multi-part name. In this
                // case, store the import info on the name part node.
                assert(!module_import.name_parts.is_empty(), None);
                assert(module_import.name_parts.len() - 1 < name_node_parts.len(), None);
                import_info.push(ImportInfoRecord {
                    node: name_node_parts[module_import.name_parts.len() - 1].node(),
                    import_result,
                });
            }
        }

        ResolveImportResult { imports, builtins_import_result, import_info }
    }

    // analyzer/sourceFile.ts:1594
    fn parse_file(
        &self,
        config_options: &ConfigOptions,
        file_uri: Uri,
        file_contents: &str,
        use_notebook_mode: bool,
        diag_sink: P<DiagnosticSink>,
        content_hash: StringFingerprint,
    ) -> ParseFileResults {
        let parse_options = SourceFile::make_parse_options(config_options, file_uri, use_notebook_mode);
        self.parse_file_contents(file_contents, parse_options, diag_sink, content_hash)
    }

    /// The options `parse_file` parses `file_uri` with.
    // analyzer/sourceFile.ts:1604
    fn make_parse_options(config_options: &ConfigOptions, file_uri: Uri, use_notebook_mode: bool) -> ParseOptions {
        let exec_environment = config_options.find_exec_environment(file_uri);

        let mut parse_options = ParseOptions::new();
        parse_options.use_notebook_mode = use_notebook_mode;
        if file_uri.path_ends_with("pyi") {
            parse_options.is_stub_file = true;
        }
        parse_options.python_version = exec_environment.python_version;
        parse_options.skip_function_and_class_body = config_options.index_generation_mode.unwrap_or(false);
        parse_options
    }

    /// The options `parse` parses this file with.
    fn parse_options(&self, config_options: &ConfigOptions) -> ParseOptions {
        SourceFile::make_parse_options(config_options, self.uri, self.ipython_mode != IPythonMode::None)
    }

    // analyzer/sourceFile.ts:1079
    fn read_file_contents(&self) -> Option<String> {
        self.file_system.read_file_text_sync(self.uri).ok()
    }

    // analyzer/sourceFile.ts:1083
    fn parse_file_contents(
        &self,
        file_contents: &str,
        parse_options: ParseOptions,
        diag_sink: P<DiagnosticSink>,
        content_hash: StringFingerprint,
    ) -> ParseFileResults {
        Parser::new().parse_source_file(JsString::from(file_contents), parse_options, diag_sink, Some(content_hash))
    }
}

// analyzer/sourceFile.ts:1392
fn add_task_list_diagnostics(
    task_list_tokens: Option<&[TaskListToken]>,
    file_contents: &[u16],
    tokenizer_output: &TokenizerOutput,
    diag_list: &mut Vec<P<Diagnostic>>,
) {
    let Some(task_list_tokens) = task_list_tokens.filter(|t| !t.is_empty()) else {
        return;
    };

    let char_code_at = |pos: i32| file_contents.get(pos as usize).copied().unwrap_or(0) as i32;

    for i in 0..tokenizer_output.tokens.count() {
        let token = tokenizer_output.tokens.get_item_at(i);

        // If there are no comments, skip this token.
        let Some(comments) = crate::parser::tokenizer_types::Token::comments(token).filter(|c| !c.is_empty()) else {
            continue;
        };

        for comment in comments {
            for task_token in task_list_tokens {
                let comment_start = comment.start;
                let comment_end = comment_start + comment.length;
                let task_text: Vec<u16> = task_token.text.encode_utf16().collect();
                let task_len = task_text.len() as i32;

                // Skip leading whitespace within the source text range.
                let mut pos = comment_start;
                while pos < comment_end {
                    let ch = char_code_at(pos);
                    if matches!(ch, 0x20 | 0x09 | 0x0a | 0x0d | 0x0c | 0x0b) {
                        pos += 1;
                    } else {
                        break;
                    }
                }

                if pos + task_len > comment_end {
                    continue;
                }

                let mut matched = true;
                for k in 0..task_len {
                    let a = char_code_at(pos + k);
                    let b = task_text[k as usize] as i32;
                    if a != b && (a | 0x20) != (b | 0x20) {
                        matched = false;
                        break;
                    }
                }
                if !matched {
                    continue;
                }

                // After the token, require whitespace-to-end or a non-word character.
                let after_pos = pos + task_len;
                if after_pos < comment_end {
                    let ch = char_code_at(after_pos);
                    let is_word = (0x61..=0x7a).contains(&ch)
                        || (0x41..=0x5a).contains(&ch)
                        || (0x30..=0x39).contains(&ch)
                        || ch == 0x5f;
                    if is_word {
                        continue;
                    }
                }

                let range_end = comment.start + comment.length;
                let range = convert_offsets_to_range(pos, range_end, &tokenizer_output.lines);

                diag_list.push(Diagnostic::with_priority(
                    DiagnosticCategory::TaskItem,
                    comment.value.js_trim().to_string(),
                    range,
                    task_token.priority,
                ));
            }
        }
    }
}

/// The checker call in `sourceFile.ts` `check()`: builds the `Checker`, runs it, and
/// returns `fileInfo.diagnosticSink.fetchAndClear()`.
///
/// The catch around it is not ported: it turns a crash into an
/// `internalTypeCheckingError` diagnostic (docs/porting.md, "exceptions").
fn run_checker(
    source_file: &SourceFile,
    import_resolver: P<ImportResolver>,
    evaluator: P<TypeEvaluator>,
    dependent_files: Option<Vec<ParserOutput>>,
    node_info_reader: P<dyn AnalyzerNodeInfoReader>,
) -> Vec<P<Diagnostic>> {
    let parser_output = source_file.data().parser_output.unwrap();
    let node_info = create_analyzer_node_info_accessor(node_info_reader.get(), None);
    let mut checker = Checker::new(import_resolver, evaluator, &parser_output, dependent_files, node_info_reader);
    checker.check();

    let file_info = node_info.get_file_info(parser_output.parse_tree.0);
    file_info.diagnostic_sink.fetch_and_clear()
}
