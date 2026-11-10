//! The hooks of the fork's `src/oracle/oracleDumper.ts`: dump state from the
//! environment, per-stage file writes and `oracle-run.txt`. Format:
//! `oracle/FORMAT.md`, "running" and "files".
//!
//! The types stage writes only with `PYRIGHT_RS_CHECKER=1`, the stand-in for
//! the fork's `program.evaluator` check until `Program` holds an evaluator.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use jsrt::{FxHashMap, JsSet};

use jsrt::P;

use super::bind::{dump_bind, number_scopes, walk_tree};
use super::diag::{OracleDiagnosticLists, dump_diagnostics};
use super::flow::dump_flow;
use crate::analyzer::analyzer_node_info::AnalyzerNodeInfoContextImpl;
use crate::parser::parse_nodes::ModuleNode;
use super::format;
use super::tokens::dump_tokenizer_output;
use super::tree::dump_parser_output;
use super::types::dump_types;
use crate::analyzer::source_file::checker_enabled;
use crate::parser::parser::ParseFileResults;
use crate::analyzer::program::Program;
use crate::common::uri::Uri;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Tokens,
    Tree,
    Bind,
    Flow,
    Types,
    Diag,
}

const ALL_STAGES: [Stage; 6] = [Stage::Tokens, Stage::Tree, Stage::Bind, Stage::Flow, Stage::Types, Stage::Diag];

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Stage::Tokens => "tokens",
            Stage::Tree => "tree",
            Stage::Bind => "bind",
            Stage::Flow => "flow",
            Stage::Types => "types",
            Stage::Diag => "diag",
        }
    }
}

struct OracleState {
    dir: PathBuf,
    stages: Vec<Stage>,
    written: JsSet<String>,
    events: Vec<String>,
    cache_empty_count: u32,
    finished: bool,
}

thread_local! {
    static STATE: RefCell<Option<Option<OracleState>>> = const { RefCell::new(None) };
}

/// Reads the environment once. The fork does this at module load, so an
/// unknown stage name stops the run before anything else happens.
pub fn init() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.is_none() {
            *state = Some(create_state());
        }
    });
}

fn create_state() -> Option<OracleState> {
    let dir = std::env::var("PYRIGHT_ORACLE_DUMP").ok().filter(|d| !d.is_empty())?;

    let mut stages = Vec::new();
    match std::env::var("PYRIGHT_ORACLE_STAGES").ok().filter(|s| !s.is_empty()) {
        Some(stage_list) => {
            for name in stage_list.split(',') {
                match ALL_STAGES.iter().find(|s| s.name() == name.trim()) {
                    Some(stage) => {
                        if !stages.contains(stage) {
                            stages.push(*stage);
                        }
                    }
                    None => fatal(&format!("unknown stage \"{name}\" in PYRIGHT_ORACLE_STAGES")),
                }
            }
        }
        None => stages.extend(ALL_STAGES),
    }

    let dir = std::path::absolute(&dir).unwrap_or_else(|e| fatal(&format!("cannot resolve {dir}: {e}")));
    Some(OracleState { dir, stages, written: JsSet::default(), events: Vec::new(), cache_empty_count: 0, finished: false })
}

fn fatal(message: &str) -> ! {
    eprintln!("pyright oracle: {message}");
    std::process::exit(70);
}

fn with_state<R>(callback: impl FnOnce(&mut OracleState) -> R) -> Option<R> {
    init();
    STATE.with(|state| state.borrow_mut().as_mut().and_then(|s| s.as_mut()).map(callback))
}

pub fn is_oracle_enabled() -> bool {
    with_state(|_| ()).is_some()
}

/// Called by `SourceFile.parse` right after the parser returns.
pub fn oracle_dump_parse(uri: Uri, results: &ParseFileResults) {
    with_state(|s| {
        if s.stages.contains(&Stage::Tokens) {
            write_dump(s, Stage::Tokens, uri, dump_tokenizer_output(&results.tokenizer_output));
        }
        if s.stages.contains(&Stage::Tree) {
            write_dump(s, Stage::Tree, uri, dump_parser_output(&results.parser_output));
        }
    });
}

/// Called by `SourceFile.bind` right after the binder's results are published.
pub fn oracle_dump_bind(uri: Uri, root: ModuleNode, reader: P<AnalyzerNodeInfoContextImpl>) {
    with_state(|s| {
        if !s.stages.contains(&Stage::Bind) && !s.stages.contains(&Stage::Flow) {
            return;
        }

        let walk = walk_tree(root.node());
        let scopes = number_scopes(&walk, reader.get());
        if s.stages.contains(&Stage::Bind) {
            write_dump(s, Stage::Bind, uri, dump_bind(&walk, &scopes, reader.get()));
        }
        if s.stages.contains(&Stage::Flow) {
            write_dump(s, Stage::Flow, uri, dump_flow(&walk, &scopes, reader.get()));
        }
    });
}

/// Called by `Program.emptyCache`. Nothing calls it: the port never empties a cache.
pub fn oracle_note_cache_emptied() {
    with_state(|s| s.cache_empty_count += 1);
}

/// Called once by the CLI when analysis is complete, before it reports.
// `printed` is keyed by `P<Type>`, which hashes by address.
#[allow(clippy::mutable_key_type)]
pub fn oracle_dump_final(program: &Program) {
    with_state(|s| {
        if s.finished {
            return;
        }
        s.finished = true;

        let mut printed = FxHashMap::default();
        for file_info in program.get_source_file_info_list() {
            let source_file = file_info.source_file;
            if source_file.is_parse_required() || source_file.get_parser_output().is_none() {
                continue;
            }

            let uri = source_file.get_uri();
            if s.stages.contains(&Stage::Types) && checker_enabled() {
                if let Some(parser_output) = source_file.get_parser_output() {
                    let text = dump_types(program, parser_output.parse_tree, &mut printed);
                    write_dump(s, Stage::Types, uri, text);
                }
            }
            if s.stages.contains(&Stage::Diag) {
                let data = source_file.data();
                let text = dump_diagnostics(&OracleDiagnosticLists {
                    parse: &data.parse_diagnostics,
                    comment: &data.comment_diagnostics,
                    bind: &data.bind_diagnostics,
                    checker: &data.checker_diagnostics,
                    task_list: &data.task_list_diagnostics,
                    final_: &data.accumulated_diagnostics,
                });
                drop(data);
                write_dump(s, Stage::Diag, uri, text);
            }
        }

        let mut run = vec![
            format!("cacheEmptyCount: {}", s.cache_empty_count),
            format!("events: [{}]", s.events.len()),
        ];
        run.extend(s.events.iter().enumerate().map(|(i, e)| format!("  [{i}]: {e}")));
        let stages: Vec<&str> = ALL_STAGES.iter().filter(|st| s.stages.contains(st)).map(|st| st.name()).collect();
        run.push(format!("stages: {}", stages.join(",")));
        create_dir_all(&s.dir);
        write_file(&s.dir.join("oracle-run.txt"), &(run.join("\n") + "\n"));
    });
}

fn write_dump(s: &mut OracleState, stage: Stage, uri: Uri, text: String) {
    let rel_path = format!("{}/{}.txt", stage.name(), format::map_path(uri.get_file_path()));
    if s.written.contains(&rel_path) {
        s.events.push(format!("rewritten {}", format::str(&rel_path)));
    }
    s.written.insert(rel_path.clone());

    let full_path = s.dir.join(&rel_path);
    create_dir_all(full_path.parent().unwrap());
    write_file(&full_path, &text);
}

fn create_dir_all(dir: &Path) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        fatal(&format!("cannot create {}: {e}", dir.display()));
    }
}

fn write_file(path: &Path, text: &str) {
    if let Err(e) = std::fs::write(path, text) {
        fatal(&format!("cannot write {}: {e}", path.display()));
    }
}
