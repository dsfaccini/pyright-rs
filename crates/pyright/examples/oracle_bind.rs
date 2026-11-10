//! Binds the files of a reference dump and writes Rust bind and flow dumps, for `oracle/compare.py`.
//!
//!     cargo run --release --example oracle_bind -- [--path PREFIX]... <reference dump dir> <out dir>
//!     cargo run --release --example oracle_bind -- [--path PREFIX]... --timing <reference dump dir>
//!
//! An interim harness until infra's `Program` runs the binder: it parses each
//! file named by `<reference>/bind/`, replays the import results that the
//! reference dump recorded (`importInfo`), and binds it the way
//! `Program._bindFile` does: builtins first, builtins scope chained, import
//! lookups bind their target on demand. It also compares each file's bind
//! diagnostics with the `bind:` list of the reference diag dump.
//!
//! `--timing` writes no dumps (and takes no out dir) and prints the time spent
//! in pyright's `--stats` phases Tokenize, Parse and Bind (the timers at
//! `parser.ts:275`, `parser.ts:283` and `sourceFile.ts:922`), the file count
//! and the arena bytes, which only grow, so the last count is the peak. File
//! reading, import replay, dump writing and the bind diagnostic comparison run
//! outside the timers.
//!
//! Options give the corpus config: `--python-version 3.11` (default 3.13),
//! `--mode strict|standard|basic|off` (default standard), and `--rule
//! <name>=<level>` (repeatable, applied after the mode). Platform is Linux.
//! `isInPyTypedPackage` is an import-resolution result, so it is replayed too:
//! true when the reference dump shows a symbol with `PrivatePyTypedImport` (512),
//! the only bind output it changes.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use jsrt::arena::arena_allocated_bytes;
use jsrt::{JsMap, JsSet, JsString, P};
use pyright::analyzer::analyzer_file_info::{AnalyzerFileInfo, ImportLookupKey, ImportLookupResult};
use pyright::analyzer::analyzer_node_info::{
    AnalyzerNodeInfoContext, AnalyzerNodeInfoContextImpl, AnalyzerNodeInfoReader,
    create_analyzer_node_info_accessor, get_dunder_all_info, get_scope,
};
use pyright::analyzer::binder::Binder;
use pyright::analyzer::comment_utils::get_file_level_directives;
use pyright::analyzer::import_result::{ImplicitImport, ImportResult, ImportType};
use pyright::analyzer::py_typed_utils::PyTypedInfo;
use pyright::analyzer::source_file::IPythonMode;
use pyright::analyzer::symbol::SymbolTable;
use pyright::common::case_sensitivity_detector::CaseSensitiveDetector;
use pyright::common::config_options::{ConfigOptions, DiagnosticLevel, DiagnosticRuleSet, ExecutionEnvironment};
use pyright::common::diagnostic_rules::DiagnosticRule;
use pyright::common::python_version::PythonVersion;
use pyright::common::timing::{TimingStat, timing_stats};
use pyright::common::diagnostic::Diagnostic;
use pyright::common::diagnostic_sink::{DiagnosticSink, TextRangeDiagnosticSink};
use pyright::common::uri::Uri;
use pyright::oracle::bind::{dump_bind, node_ref, number_scopes, walk_tree};
use pyright::oracle::flow::dump_flow;
use pyright::oracle::format;
use pyright::parser::parse_nodes::{ModuleNode, ParseNode};
use pyright::parser::parser::{ParseOptions, Parser};

/// One `<key>: <value>` or `<key>:` line of a dump, with its nested lines.
struct DumpNode {
    key: String,
    value: Option<String>,
    children: Vec<DumpNode>,
}

impl DumpNode {
    fn child(&self, key: &str) -> Option<&DumpNode> {
        self.children.iter().find(|c| c.key == key)
    }

    fn value_of(&self, key: &str) -> Option<&str> {
        self.child(key).and_then(|c| c.value.as_deref())
    }
}

fn parse_dump(text: &str) -> Vec<DumpNode> {
    let mut stack: Vec<(usize, DumpNode)> = Vec::new();
    let mut roots = Vec::new();
    let finish = |stack: &mut Vec<(usize, DumpNode)>, roots: &mut Vec<DumpNode>| {
        let (_, node) = stack.pop().unwrap();
        match stack.last_mut() {
            Some((_, parent)) => parent.children.push(node),
            None => roots.push(node),
        }
    };
    for line in text.lines() {
        let depth = (line.len() - line.trim_start_matches(' ').len()) / 2;
        let content = &line[depth * 2..];
        let (key, value) = match content.find(": ") {
            Some(i) if !content.starts_with('"') || content[..i].ends_with('"') => {
                (content[..i].to_string(), Some(content[i + 2..].to_string()))
            }
            _ => (content.trim_end_matches(':').to_string(), None),
        };
        while stack.last().is_some_and(|(d, _)| *d >= depth) {
            finish(&mut stack, &mut roots);
        }
        stack.push((
            depth,
            DumpNode {
                key,
                value,
                children: Vec::new(),
            },
        ));
    }
    while !stack.is_empty() {
        finish(&mut stack, &mut roots);
    }
    roots
}

fn json_string(value: &str) -> String {
    serde_json::from_str::<String>(value).unwrap_or_else(|_| value.trim_matches('"').to_string())
}

struct Harness {
    python_version: PythonVersion,
    rule_set: DiagnosticRuleSet,
    reference: PathBuf,
    /// `None` with `--timing`: no dumps.
    out: Option<PathBuf>,
    fork: PathBuf,
    context: P<AnalyzerNodeInfoContextImpl>,
    files: RefCell<HashMap<String, Rc<FileState>>>,
    bind_diag_mismatches: Cell<usize>,
}

struct FileState {
    binding: Cell<bool>,
    bound: Cell<bool>,
    module: Cell<Option<ModuleNode>>,
    file_info: Cell<Option<P<AnalyzerFileInfo>>>,
}

const BUILTINS: &str = "typeshed/stdlib/builtins.pyi";

impl Harness {
    fn source_path(&self, mapped: &str) -> Option<PathBuf> {
        if let Some(rest) = mapped.strip_prefix("typeshed/") {
            return Some(
                self.fork
                    .join("packages/pyright/dist/typeshed-fallback")
                    .join(rest),
            );
        }
        mapped
            .strip_prefix("fs/")
            .map(|rest| PathBuf::from("/").join(rest))
    }

    fn uri_from_dump(&self, value: &str) -> Uri {
        let text = json_string(value.strip_prefix("Uri(").unwrap().strip_suffix(')').unwrap());
        if text.is_empty() {
            return Uri::empty();
        }
        match self.source_path(&text) {
            Some(path) => Uri::file(&path.to_string_lossy(), &CaseSensitiveDetector, false),
            None => panic!("cannot map Uri {text}"),
        }
    }

    fn py_typed_info(&self, node: &DumpNode) -> PyTypedInfo {
        PyTypedInfo {
            is_partially_typed: node.value_of("isPartiallyTyped") == Some("true"),
            py_typed_path: self.uri_from_dump(node.value_of("pyTypedPath").unwrap()),
        }
    }

    fn implicit_imports(&self, node: &DumpNode) -> JsMap<String, ImplicitImport> {
        let mut map = JsMap::default();
        for entry in &node.children {
            map.insert(
                json_string(&entry.key),
                ImplicitImport {
                    is_stub_file: entry.value_of("isStubFile") == Some("true"),
                    is_native_lib: entry.value_of("isNativeLib") == Some("true"),
                    name: json_string(entry.value_of("name").unwrap()),
                    uri: self.uri_from_dump(entry.value_of("uri").unwrap()),
                    py_typed_info: entry.child("pyTypedInfo").map(|n| self.py_typed_info(n)),
                },
            );
        }
        map
    }

    fn import_result(&self, node: &DumpNode) -> ImportResult {
        let flag = |key: &str| node.value_of(key) == Some("true");
        let opt_flag = |key: &str| node.value_of(key).map(|v| v == "true");
        ImportResult {
            import_name: json_string(node.value_of("importName").unwrap()),
            is_relative: flag("isRelative"),
            is_import_found: flag("isImportFound"),
            is_partly_resolved: flag("isPartlyResolved"),
            is_namespace_package: flag("isNamespacePackage"),
            is_init_file_present: flag("isInitFilePresent"),
            is_stub_package: flag("isStubPackage"),
            import_failure_info: None,
            import_type: match node.value_of("importType").unwrap() {
                "0" => ImportType::BuiltIn,
                "1" => ImportType::ThirdParty,
                _ => ImportType::Local,
            },
            resolved_uris: node
                .child("resolvedUris")
                .map(|n| {
                    n.children
                        .iter()
                        .map(|c| self.uri_from_dump(c.value.as_deref().unwrap()))
                        .collect()
                })
                .unwrap_or_default(),
            search_path: node.value_of("searchPath").map(|v| self.uri_from_dump(v)),
            is_stub_file: flag("isStubFile"),
            is_native_lib: flag("isNativeLib"),
            is_stdlib_typeshed_file: opt_flag("isStdlibTypeshedFile"),
            is_third_party_typeshed_file: opt_flag("isThirdPartyTypeshedFile"),
            is_local_typings_file: opt_flag("isLocalTypingsFile"),
            implicit_imports: node.child("implicitImports").map(|n| self.implicit_imports(n)),
            filtered_implicit_imports: node
                .child("filteredImplicitImports")
                .map(|n| self.implicit_imports(n)),
            non_stub_import_result: node
                .child("nonStubImportResult")
                .map(|n| P::new(self.import_result(n))),
            py_typed_info: node.child("pyTypedInfo").map(|n| self.py_typed_info(n)),
            package_directory: node.value_of("packageDirectory").map(|v| self.uri_from_dump(v)),
        }
    }

    fn state(&self, mapped: &str) -> Rc<FileState> {
        self.files
            .borrow_mut()
            .entry(mapped.to_string())
            .or_insert_with(|| {
                Rc::new(FileState {
                    binding: Cell::new(false),
                    bound: Cell::new(false),
                    module: Cell::new(None),
                    file_info: Cell::new(None),
                })
            })
            .clone()
    }

    /// Whether a symbol of the reference bind dump has `PrivatePyTypedImport`.
    fn is_in_py_typed_package(reference_text: &str) -> bool {
        reference_text.lines().any(|line| {
            line.trim_start()
                .strip_prefix("_flags: ")
                .and_then(|flags| flags.parse::<u32>().ok())
                .is_some_and(|flags| flags & 512 != 0)
        })
    }

    fn import_lookup(self: &Rc<Self>, key: ImportLookupKey, skip_parsing: bool) -> Option<ImportLookupResult> {
        let ImportLookupKey::Uri(uri) = key else {
            return None;
        };
        if uri.is_empty() {
            return None;
        }
        let mapped = format::map_path(uri.get_file_path());
        if !self.reference.join("bind").join(format!("{mapped}.txt")).exists() {
            return None;
        }
        if skip_parsing {
            return Some(ImportLookupResult {
                symbol_table: SymbolTable::new(),
                dunder_all_names: None,
                uses_unsupported_dunder_all_form: false,
                doc_string: Rc::new(|| None),
                is_in_py_typed_package: false,
            });
        }
        let state = self.state(&mapped);
        if state.binding.get() {
            eprintln!("oracle_bind: cyclic import lookup of {mapped}");
            return None;
        }
        self.bind(&mapped);
        let module = state.module.get()?;
        let reader: &dyn AnalyzerNodeInfoReader = self.context.get();
        let scope = get_scope(module, reader)?;
        let dunder_all_info = get_dunder_all_info(module, reader);
        Some(ImportLookupResult {
            symbol_table: scope.symbol_table,
            dunder_all_names: dunder_all_info.map(|info| info.names.clone()),
            uses_unsupported_dunder_all_form: dunder_all_info
                .is_some_and(|info| info.uses_unsupported_dunder_all_form),
            doc_string: Rc::new(|| None),
            is_in_py_typed_package: state
                .file_info
                .get()
                .is_some_and(|info| info.is_in_py_typed_package),
        })
    }

    fn bind(self: &Rc<Self>, mapped: &str) {
        let state = self.state(mapped);
        if state.bound.get() || state.binding.get() {
            return;
        }

        // Program._bindFile: bind the implicit imports (builtins) first.
        if mapped != BUILTINS && self.reference.join("bind").join(format!("{BUILTINS}.txt")).exists() {
            self.bind(BUILTINS);
        }
        if state.bound.get() {
            return;
        }
        state.binding.set(true);
        let bind_time = &timing_stats().bind_time;

        // A file bound from an import lookup runs inside the importer's Bind
        // timer, as in pyright. Reading and replay stay outside it.
        let source = self.source_path(mapped).expect("source path");
        let source_text = source.to_string_lossy().into_owned();
        let text = outside_timer(bind_time, || {
            let bytes = fs::read(&source).unwrap_or_else(|e| panic!("cannot read {}: {e}", source.display()));
            let text: Vec<u16> = String::from_utf8_lossy(&bytes).encode_utf16().collect();
            JsString::from(text)
        });
        let file_uri = Uri::file(&source_text, &CaseSensitiveDetector, false);
        let is_stub_file = source_text.ends_with("pyi");

        let parse_options = ParseOptions {
            is_stub_file,
            python_version: self.python_version,
            ..ParseOptions::new()
        };
        let parse_results =
            Parser::new().parse_source_file(text, parse_options, P::new(DiagnosticSink::new(None)), None);
        let parse_tree = parse_results.parser_output.parse_tree;
        let lines = parse_results.parser_output.lines;

        // The reference dump: import results to replay, the module name and
        // `isInPyTypedPackage`.
        let (walk, import_info, module_name, is_in_py_typed_package) = outside_timer(bind_time, || {
            let reference_text = fs::read_to_string(self.reference.join("bind").join(format!("{mapped}.txt")))
                .unwrap_or_default();
            let reference = parse_dump(&reference_text);
            let mut by_ref: HashMap<String, P<ParseNode>> = HashMap::new();
            let walk = walk_tree(parse_tree.node());
            for node in &walk {
                by_ref.entry(node_ref(node)).or_insert(*node);
            }
            let mut import_info: Vec<(P<ParseNode>, P<ImportResult>)> = Vec::new();
            let mut module_name = String::new();
            for section in &reference {
                if section.key == "nodes" {
                    for entry in &section.children {
                        if let Some(info) = entry.child("importInfo") {
                            match by_ref.get(&entry.key) {
                                Some(node) => import_info.push((*node, P::new(self.import_result(info)))),
                                None => eprintln!("oracle_bind: {mapped}: no node {}", entry.key),
                            }
                        }
                    }
                }
                if section.key == "scopes" {
                    let name_decl = section
                        .child("[0]")
                        .and_then(|s| s.child("symbolTable"))
                        .and_then(|t| t.child("\"__name__\""))
                        .and_then(|s| s.child("_declarations"))
                        .and_then(|d| d.child("[0]"));
                    if let Some(value) = name_decl.and_then(|d| d.value_of("moduleName")) {
                        module_name = json_string(value);
                    }
                }
            }
            (walk, import_info, module_name, Self::is_in_py_typed_package(&reference_text))
        });

        // SourceFile.parse: the file's `# pyright:` comments adjust the rule set.
        let env_rule_set = self.rule_set.clone();
        let mut comment_diagnostics = Vec::new();
        let rule_set = get_file_level_directives(
            &parse_results.tokenizer_output.tokens,
            lines,
            &env_rule_set,
            false,
            &mut comment_diagnostics,
        );

        // sourceFile.ts:922: the Bind timer.
        let (file_info, bind_diagnostics) = bind_time.time_operation(|| {
            let session = self.context.begin_write(parse_tree);
            let node_info = create_analyzer_node_info_accessor(session.get(), Some(session.get()));
            for (node, import_result) in &import_info {
                node_info.set_import_info(*node, *import_result);
            }

            let builtins_scope = if mapped == BUILTINS {
                None
            } else {
                self.state(BUILTINS)
                    .module
                    .get()
                    .and_then(|m| get_scope(m, self.context.get()))
            };

            let path_ends_with = |suffix: &str| source_text.ends_with(suffix);
            let file_name = source.file_name().unwrap().to_string_lossy().into_owned();
            let is_built_in_stub_file = is_stub_file
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
                    "stdlib/types.pyi",
                    "stdlib/warnings.pyi",
                ]
                .iter()
                .any(|s| path_ends_with(s));

            let harness = self.clone();
            let file_info = P::new(AnalyzerFileInfo {
                import_lookup: Rc::new(move |key, options| {
                    harness.import_lookup(key, options.is_some_and(|o| o.skip_parsing == Some(true)))
                }),
                future_imports: JsSet::default(),
                builtins_scope,
                diagnostic_sink: TextRangeDiagnosticSink::new(lines, None),
                execution_environment: P::new(ExecutionEnvironment::new(
                    "python".to_string(),
                    Uri::empty(),
                    &env_rule_set,
                    Some(self.python_version),
                    Some("Linux".to_string()),
                    None,
                    false,
                )),
                diagnostic_rule_set: rule_set,
                lines,
                typing_symbol_aliases: parse_results.parser_output.typing_symbol_aliases.clone(),
                defined_constants: JsMap::default(),
                file_id: String::new(),
                file_uri,
                module_name,
                is_stub_file,
                is_typing_stub_file: is_stub_file
                    && (path_ends_with("stdlib/typing.pyi") || file_name == "typing_extensions.pyi"),
                is_typing_extensions_stub_file: is_stub_file && file_name == "typing_extensions.pyi",
                is_typeshed_stub_file: is_stub_file
                    && (path_ends_with("stdlib/_typeshed/__init__.pyi")
                        || path_ends_with("stdlib/_typeshed/_type_checker_internals.pyi")),
                is_built_in_stub_file,
                is_in_py_typed_package,
                ipython_mode: IPythonMode::None,
                accessed_symbol_set: Default::default(),
            });
            node_info.set_file_info(parse_tree, file_info);

            let mut binder = Binder::new(file_info, None, None, node_info);
            binder.bind_module(parse_tree);
            let bind_diagnostics = file_info.diagnostic_sink.fetch_and_clear();
            self.context.publish(session);
            (file_info, bind_diagnostics)
        });

        state.module.set(Some(parse_tree));
        state.file_info.set(Some(file_info));
        state.binding.set(false);
        state.bound.set(true);

        bind_time.subtract_from_time(|| {
            if let Some(out) = &self.out {
                let reader: &dyn AnalyzerNodeInfoReader = self.context.get();
                let numbering = number_scopes(&walk, reader);
                for (stage, text) in [
                    ("bind", dump_bind(&walk, &numbering, reader)),
                    ("flow", dump_flow(&walk, &numbering, reader)),
                ] {
                    let target = out.join(stage).join(format!("{mapped}.txt"));
                    fs::create_dir_all(target.parent().unwrap()).unwrap();
                    fs::write(&target, text).unwrap();
                }
            }

            self.compare_bind_diagnostics(mapped, &bind_diagnostics);
        });
    }

    fn compare_bind_diagnostics(&self, mapped: &str, diagnostics: &[P<Diagnostic>]) {
        let Ok(reference) = fs::read_to_string(self.reference.join("diag").join(format!("{mapped}.txt"))) else {
            return;
        };
        let expected: Vec<&str> = reference
            .lines()
            .skip_while(|l| !l.starts_with("bind: "))
            .take_while(|l| !l.starts_with("checker: "))
            .collect();
        let mut actual = vec![format!("bind: [{}]", diagnostics.len())];
        for (i, diag) in diagnostics.iter().enumerate() {
            let range = diag.range;
            actual.push(format!(
                "  [{i}]: {} {}:{}-{}:{} {} {}",
                diag.category as i32,
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character,
                diag.get_rule().unwrap_or_else(|| "-".to_string()),
                format::str(&diag.message)
            ));
        }
        if expected != actual.iter().map(String::as_str).collect::<Vec<_>>() {
            self.bind_diag_mismatches.set(self.bind_diag_mismatches.get() + 1);
            eprintln!("oracle_bind: {mapped}: bind diagnostics differ");
            for line in &expected {
                eprintln!("  - {line}");
            }
            for line in &actual {
                eprintln!("  + {line}");
            }
        }
    }
}

/// `TimingStat::subtract_from_time` for a callback that returns a value.
fn outside_timer<R>(stat: &TimingStat, callback: impl FnOnce() -> R) -> R {
    let mut result = None;
    stat.subtract_from_time(|| result = Some(callback()));
    result.unwrap()
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, files)?;
        } else if path.extension().is_some_and(|e| e == "txt") {
            files.push(path);
        }
    }
    Ok(())
}

fn run() -> ExitCode {
    let mut prefixes: Vec<String> = vec![];
    let mut positional: Vec<String> = vec![];
    let mut python_version = PythonVersion::from_string("3.13").unwrap();
    let mut mode = "standard".to_string();
    let mut rules: Vec<(DiagnosticRule, DiagnosticLevel)> = vec![];
    let mut timing = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--path" => prefixes.push(args.next().expect("--path needs a value")),
            "--timing" => timing = true,
            "--python-version" => {
                python_version =
                    PythonVersion::from_string(&args.next().expect("--python-version needs a value")).expect("version")
            }
            "--mode" => mode = args.next().expect("--mode needs a value"),
            "--rule" => {
                let rule = args.next().expect("--rule needs name=level");
                let (name, level) = rule.split_once('=').expect("--rule needs name=level");
                let level = match level {
                    "false" => DiagnosticLevel::None,
                    "true" => DiagnosticLevel::Error,
                    other => DiagnosticLevel::from_name(other).expect("level"),
                };
                rules.push((DiagnosticRule::from_name(name).expect("rule name"), level));
            }
            _ => positional.push(arg),
        }
    }
    let mut rule_set = ConfigOptions::get_diagnostic_rule_set(Some(&mode));
    for (rule, level) in rules {
        rule_set.set_level(rule, level);
    }
    let (reference, out) = match (positional.as_slice(), timing) {
        ([reference], true) => (reference, None),
        ([reference, out], false) => (reference, Some(PathBuf::from(out))),
        _ => {
            eprintln!("usage: oracle_bind [options] <reference dump dir> <out dir>");
            eprintln!("       oracle_bind [options] --timing <reference dump dir>");
            return ExitCode::from(2);
        }
    };
    let fork = std::env::var_os("PYRIGHT_ORACLE_FORK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").expect("HOME")).join("pyright-oracle"));
    let harness = Rc::new(Harness {
        python_version,
        rule_set,
        reference: PathBuf::from(reference),
        out,
        fork,
        context: AnalyzerNodeInfoContextImpl::new(),
        files: RefCell::new(HashMap::new()),
        bind_diag_mismatches: Cell::new(0),
    });

    let bind_dir = harness.reference.join("bind");
    let mut files = vec![];
    collect(&bind_dir, &mut files).expect("list reference bind dir");
    files.sort();
    let mut count = 0;
    for file in &files {
        let relative = file.strip_prefix(&bind_dir).unwrap().to_string_lossy().into_owned();
        let mapped = relative.strip_suffix(".txt").unwrap().to_string();
        if !prefixes.is_empty() && !prefixes.iter().any(|p| mapped.starts_with(p.as_str())) {
            continue;
        }
        harness.bind(&mapped);
        count += 1;
    }
    eprintln!(
        "oracle_bind: bound {count} files, {} bind diagnostic lists differ",
        harness.bind_diag_mismatches.get()
    );
    if timing {
        let stats = timing_stats();
        let bound = harness.files.borrow().values().filter(|state| state.bound.get()).count();
        println!("Files:                {bound}");
        for (name, stat) in [
            ("Tokenize:             ", &stats.tokenize_file_time),
            ("Parse:                ", &stats.parse_file_time),
            ("Bind:                 ", &stats.bind_time),
        ] {
            println!("{name}{} ({:.1} ms)", stat.print_time(), stat.total_time.get());
        }
        println!("Peak arena bytes:     {}", arena_allocated_bytes());
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap()
}
