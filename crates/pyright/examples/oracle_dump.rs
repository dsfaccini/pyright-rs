//! Writes Rust oracle dumps for the files of a reference dump, for `oracle/compare.py`.
//!
//!     cargo run --release --example oracle_dump -- [--python-version <x.y>] [--stage tokens|tree]... <reference dump dir> <out dir>
//!
//! For every `<reference>/<stage>/<mapped path>.txt`, reads the source file the
//! mapped path names (`typeshed/...` under the fork's bundled typeshed, `fs/...`
//! as an absolute path), runs the Rust stage, and writes `<out>/<stage>/<mapped path>.txt`.
//! The fork defaults to `~/pyright-oracle`; set `PYRIGHT_ORACLE_FORK` to change it.
//! Without `--stage`, runs every stage this tool supports. `--python-version`
//! is the corpus's configured version, 3.13 when omitted.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use jsrt::{JsString, P};
use pyright::common::diagnostic_sink::DiagnosticSink;
use pyright::common::python_version::{PYTHON_VERSION3_13, PythonVersion};
use pyright::oracle::tokens::dump_tokenizer_output;
use pyright::oracle::tree::dump_parser_output;
use pyright::parser::parser::{ParseOptions, Parser};
use pyright::parser::tokenizer::Tokenizer;

const STAGES: [&str; 2] = ["tokens", "tree"];

/// The parse options `SourceFile.parse` builds: the corpus's Python version, and
/// a `.pyi` path parses as a stub.
fn parse_options(mapped: &str, python_version: PythonVersion) -> ParseOptions {
    let mut options = ParseOptions::new();
    options.is_stub_file = mapped.ends_with("pyi");
    options.python_version = python_version;
    options
}

/// The source file a mapped dump path names, or `None` for an `other/` path.
fn source_path(mapped: &str, fork: &Path) -> Option<PathBuf> {
    if let Some(rest) = mapped.strip_prefix("typeshed/") {
        return Some(
            fork.join("packages/pyright/dist/typeshed-fallback")
                .join(rest),
        );
    }
    mapped
        .strip_prefix("fs/")
        .map(|rest| PathBuf::from("/").join(rest))
}

/// Node `fs.readFileSync(path, 'utf8')`: UTF-8 with replacement characters, BOM kept.
fn read_source(path: &Path) -> std::io::Result<Vec<u16>> {
    let bytes = fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).encode_utf16().collect())
}

fn dump(stage: &str, mapped: &str, text: &[u16], python_version: PythonVersion) -> String {
    match stage {
        "tokens" => {
            let output =
                Tokenizer::new().tokenize(text, Some(0), Some(text.len() as i32), 0, false);
            dump_tokenizer_output(&output)
        }
        "tree" => {
            let results = Parser::new().parse_source_file(
                JsString::from(text.to_vec()),
                parse_options(mapped, python_version),
                P::new(DiagnosticSink::new(None)),
                None,
            );
            dump_parser_output(&results.parser_output)
        }
        _ => unreachable!(),
    }
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

fn main() -> ExitCode {
    // The parser recurses once per nesting level, as the TypeScript does.
    std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap()
}

fn run() -> ExitCode {
    let mut stages: Vec<String> = vec![];
    let mut positional: Vec<String> = vec![];
    let mut python_version = PYTHON_VERSION3_13;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--python-version" {
            let Some(version) = args.next().and_then(|v| PythonVersion::from_string(&v)) else {
                eprintln!("oracle_dump: --python-version needs a version such as 3.11");
                return ExitCode::from(2);
            };
            python_version = version;
        } else if arg == "--stage" {
            let Some(stage) = args.next() else {
                eprintln!("oracle_dump: --stage needs a value");
                return ExitCode::from(2);
            };
            if !STAGES.contains(&stage.as_str()) {
                eprintln!(
                    "oracle_dump: unsupported stage {stage}; supported: {}",
                    STAGES.join(", ")
                );
                return ExitCode::from(2);
            }
            stages.push(stage);
        } else {
            positional.push(arg);
        }
    }
    if stages.is_empty() {
        stages = STAGES.iter().map(|s| s.to_string()).collect();
    }
    let [reference, out] = positional.as_slice() else {
        eprintln!(
            "usage: oracle_dump [--python-version <x.y>] [--stage tokens|tree]... <reference dump dir> <out dir>"
        );
        return ExitCode::from(2);
    };
    let reference = PathBuf::from(reference);
    let out = PathBuf::from(out);
    let fork = std::env::var_os("PYRIGHT_ORACLE_FORK")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").expect("HOME")).join("pyright-oracle")
        });

    let mut failures = 0;
    for stage in &stages {
        let stage_dir = reference.join(stage);
        let mut files = vec![];
        if let Err(e) = collect(&stage_dir, &mut files) {
            eprintln!("oracle_dump: cannot list {}: {e}", stage_dir.display());
            return ExitCode::from(1);
        }
        files.sort();
        let mut written = 0;
        for file in &files {
            let relative = file
                .strip_prefix(&stage_dir)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let mapped = relative.strip_suffix(".txt").unwrap();
            let Some(source) = source_path(mapped, &fork) else {
                eprintln!("oracle_dump: skipping {mapped}: no source path for it");
                failures += 1;
                continue;
            };
            let text = match read_source(&source) {
                Ok(text) => text,
                Err(e) => {
                    eprintln!("oracle_dump: cannot read {}: {e}", source.display());
                    failures += 1;
                    continue;
                }
            };
            let target = out.join(stage).join(&relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, dump(stage, mapped, &text, python_version)).unwrap();
            written += 1;
        }
        eprintln!(
            "oracle_dump: {stage}: wrote {written} of {} files",
            files.len()
        );
    }

    if failures > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
