//! Reads, tokenizes and parses source files on worker threads ahead of the checker
//! (`docs/porting.md`, "parse ahead"). Ports no pyright file.
//!
//! The main thread requests files it will parse; a worker reads one with the
//! file-system calls `SourceFile.parse` makes and parses it as `SourceFile.parse`
//! would. The main thread still parses each file at pyright's point
//! (`Program._parseFile`, `program.ts:2008`), and there it takes the worker's
//! result instead of reading and parsing, waits for a worker that is still at
//! it, or parses the file itself when no worker has started on it.
//!
//! A result is indistinguishable from a main-thread parse: atoms come from one
//! process-wide table (`jsrt::atom`), a file's nodes get one block of node ids
//! (`with_node_id_block`), and a worker allocates nothing a collector traces
//! (`jsrt::heap::forbid_collected_allocation`). `PYRIGHT_RS_PARSE_AHEAD=check`
//! parses each taken file on the main thread too and panics when the two differ.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use jsrt::{Atom, FxHashMap, FxHashSet, JsStr, JsString, P, SwitchMode};

use crate::analyzer::program::Program;
use crate::analyzer::source_file::MAX_SOURCE_FILE_SIZE;
use crate::analyzer::source_file_info::{SourceFileInfo, is_user_code};
use crate::common::uri::Uri;
use crate::common::diagnostic_sink::DiagnosticSink;
use crate::common::string_utils::{StringFingerprint, get_string_fingerprint};
use crate::oracle::bind::walk_tree;
use crate::oracle::tokens::dump_tokenizer_output;
use crate::oracle::tree::dump_parser_output;
use crate::parser::parse_nodes::{
    FunctionNode, ImportAsNode, ImportFromAsNode, ImportFromNode, MemberAccessNode, NameNode, ParseNode, with_node_id_block,
};
use crate::parser::parser::{ParseFileResults, ParseOptions, Parser};

/// `PYRIGHT_RS_PARSE_AHEAD`: `on` (the default) parses on workers, `off` parses
/// every file on the main thread as pyright does, `check` also parses each file a
/// worker parsed on the main thread and panics when the results differ.
pub fn mode() -> SwitchMode {
    jsrt::switch_mode!("PYRIGHT_RS_PARSE_AHEAD")
}

/// `PYRIGHT_RS_PARSE_AHEAD_THREADS`: the number of workers, default 4; 0 starts
/// none, so every file parses on the main thread, as with the switch off.
fn thread_count() -> usize {
    static COUNT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *COUNT.get_or_init(|| match std::env::var("PYRIGHT_RS_PARSE_AHEAD_THREADS") {
        Err(_) => 4,
        Ok(v) => v.parse().unwrap_or_else(|_| panic!("PYRIGHT_RS_PARSE_AHEAD_THREADS must be a number, not {v:?}")),
    })
}

/// How a worker reads and parses one file: what `SourceFile.parse` would pass.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// The file `stat_sync` and `read_file_text_sync` read (`real_file_path`).
    pub path: String,
    pub options: ParseOptions,
}

/// What `SourceFile.parse` reads and parses before it resolves imports.
pub struct Parsed {
    pub contents: String,
    pub content_hash: StringFingerprint,
    /// `contents.length`, in UTF-16 code units.
    pub content_length: i32,
    pub diag_sink: P<DiagnosticSink>,
    pub results: ParseFileResults,
}

/// A worker's `Parsed`, on its way to the main thread, with the file's module reads.
struct Handoff(Box<Parsed>, ModuleReads);

// SAFETY: a worker hands a `Parsed` over once and never touches it again. Its
// handles point into the worker's program arena, which is never freed, or into
// the token region the `TokenizerOutput` owns and takes along; none points into a
// collected heap, since the worker may allocate no object a collector traces
// (`forbid_collected_allocation`). Its atoms come from the process-wide table.
// Every `Rc` in it was made by this parse and is held only from within the
// result: the parser that made them dropped on the worker before the handoff.
// The job's mutex orders the worker's writes before the main thread's reads.
unsafe impl Send for Handoff {}

enum State {
    Queued,
    Running,
    Done(Outcome),
    /// The main thread took the job; a worker that finds it queued skips it.
    Taken,
}

enum Outcome {
    Parsed(Handoff),
    /// The worker could not read the file, which the main thread then reads and
    /// parses itself, so it reports the failure as pyright does.
    Unread,
    /// The parse panicked: a port bug, which the main thread's own parse of the
    /// file reproduces (check mode panics at once).
    Panicked,
}

struct Job {
    request: Request,
    state: Mutex<State>,
    done: Condvar,
}

#[derive(Default)]
struct Queue {
    /// Imports of a file the main thread just parsed: wanted soon, newest first.
    urgent: VecDeque<Arc<Job>>,
    /// The user files the check loop reaches next, in its order.
    later: VecDeque<Arc<Job>>,
}

struct Shared {
    queue: Mutex<Queue>,
    work: Condvar,
    /// Nanoseconds the workers spent reading and parsing.
    busy_ns: AtomicU64,
    /// Bytes of the workers' program arenas.
    arena_bytes: AtomicUsize,
    /// Results the main thread took. It frees their file texts and scratch buffers,
    /// which go back to the worker that allocated them only once that worker collects.
    taken: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
struct Stats {
    requested: u64,
    ready: u64,
    waited: u64,
    wait_ns: u64,
    stolen: u64,
    unrequested: u64,
    unread: u64,
    panicked: u64,
    stale: u64,
}

/// The main thread's side.
struct Ahead {
    shared: Arc<Shared>,
    /// Requested files by `Uri::key`, until the main thread parses them.
    jobs: FxHashMap<&'static str, Arc<Job>>,
    /// Urgent requests `flush` has yet to queue.
    pending: Vec<Arc<Job>>,
    /// The names of each module that the check will look up, as far as the import
    /// statements of the files parsed so far tell (`after_parse`).
    wanted: FxHashMap<&'static str, Wanted>,
    /// The module reads of the file `take` returned last.
    last_reads: Option<(&'static str, ModuleReads)>,
    stats: Stats,
}

thread_local! {
    static AHEAD: std::cell::RefCell<Option<Ahead>> = const { std::cell::RefCell::new(None) };
}

/// Whether this thread started workers, so it parses each file in a node id block.
pub fn active() -> bool {
    AHEAD.with(|ahead| ahead.borrow().is_some())
}

/// Starts the workers, once per thread that checks; does nothing with the switch off.
pub fn start() {
    if mode() == SwitchMode::Off || thread_count() == 0 || active() {
        return;
    }
    let shared = Arc::new(Shared {
        queue: Mutex::new(Queue::default()),
        work: Condvar::new(),
        busy_ns: AtomicU64::new(0),
        arena_bytes: AtomicUsize::new(0),
        taken: AtomicU64::new(0),
    });
    for i in 0..thread_count() {
        let shared = shared.clone();
        std::thread::Builder::new()
            .name(format!("parse-ahead-{i}"))
            // The parser recurses as deeply as on the main thread (`pyright::main`).
            .stack_size(1 << 30)
            .spawn(move || work(&shared))
            .expect("spawn a parse-ahead worker");
    }
    AHEAD.with(|ahead| {
        *ahead.borrow_mut() = Some(Ahead {
            shared,
            jobs: FxHashMap::default(),
            pending: Vec::new(),
            wanted: FxHashMap::default(),
            last_reads: None,
            stats: Stats::default(),
        })
    });
}

/// Asks a worker to parse the file `key` names, after the files already waiting;
/// an urgent request waits for `flush`, which puts it before them. Does nothing
/// when no workers run, the file is already requested or is no Python file.
pub fn request(key: &'static str, request: Request, urgent: bool) {
    AHEAD.with(|ahead| {
        let mut ahead = ahead.borrow_mut();
        let Some(ahead) = ahead.as_mut() else { return };
        // An import can resolve to a native module, which pyright never parses.
        if ahead.jobs.contains_key(key) || !(request.path.ends_with(".py") || request.path.ends_with(".pyi")) {
            return;
        }
        let job = Arc::new(Job { request, state: Mutex::new(State::Queued), done: Condvar::new() });
        ahead.jobs.insert(key, job.clone());
        ahead.stats.requested += 1;
        if urgent {
            ahead.pending.push(job);
            return;
        }
        lock(&ahead.shared.queue).later.push_back(job);
        ahead.shared.work.notify_one();
    });
}

/// Puts the urgent requests made since the last flush before every waiting job,
/// in the order they were made.
pub fn flush() {
    AHEAD.with(|ahead| {
        let mut ahead = ahead.borrow_mut();
        let Some(ahead) = ahead.as_mut() else { return };
        if ahead.pending.is_empty() {
            return;
        }
        let mut queue = lock(&ahead.shared.queue);
        for job in ahead.pending.drain(..).rev() {
            queue.urgent.push_front(job);
        }
        drop(queue);
        ahead.shared.work.notify_all();
    });
}

/// The worker's result for the file `key` names, if a worker read it as `now`
/// says the main thread would. Waits for a worker that started on it; a job no
/// worker started is withdrawn, and the caller parses the file.
pub fn take(key: &'static str, now: Option<&Request>) -> Option<Parsed> {
    AHEAD.with(|ahead| {
        let mut ahead = ahead.borrow_mut();
        let ahead = ahead.as_mut()?;
        let Some(job) = ahead.jobs.remove(key) else {
            ahead.stats.unrequested += 1;
            return None;
        };
        let mut state = lock(&job.state);
        if matches!(*state, State::Queued) {
            *state = State::Taken;
            ahead.stats.stolen += 1;
            return None;
        }
        if matches!(*state, State::Running) {
            let start = Instant::now();
            while matches!(*state, State::Running) {
                state = job.done.wait(state).unwrap_or_else(PoisonError::into_inner);
            }
            ahead.stats.waited += 1;
            ahead.stats.wait_ns += start.elapsed().as_nanos() as u64;
        } else {
            ahead.stats.ready += 1;
        }
        let State::Done(done) = std::mem::replace(&mut *state, State::Taken) else {
            unreachable!("a started job ends done");
        };
        // A file read from another path, or parsed with other options, is not this parse.
        if now != Some(&job.request) {
            ahead.stats.stale += 1;
            return None;
        }
        match done {
            Outcome::Parsed(Handoff(parsed, reads)) => {
                ahead.shared.taken.fetch_add(1, Relaxed);
                ahead.last_reads = Some((key, reads));
                Some(*parsed)
            }
            Outcome::Unread => {
                ahead.stats.unread += 1;
                None
            }
            Outcome::Panicked => {
                assert!(mode() != SwitchMode::Check, "PYRIGHT_RS_PARSE_AHEAD=check: {}: the worker's parse panicked", job.request.path);
                ahead.stats.panicked += 1;
                None
            }
        }
    })
}

/// Names of a module that the check will look up.
#[derive(Clone)]
enum Wanted {
    All,
    Names(FxHashSet<String>),
}

/// Requests the files the check will parse soon because of `file`, which the main
/// thread just parsed and whose imports it resolved (`Program._parseFile`).
///
/// Checking a user file resolves each of its imports, and each name it imports
/// from a module, and a name that module imports from another module in turn.
/// So a user file wants every name, a module wants the names some wanted import
/// statement imports from it, and a module is requested once some file imports a
/// wanted name from it, or a user file imports it. A name that a later file wants
/// from a module parsed already requests what that module imports it from.
/// Evaluation needs more files than this finds, which the main thread parses itself.
pub fn after_parse(program: &Program, file: P<SourceFileInfo>) {
    if !active() {
        return;
    }
    let key = file.uri().key();
    let (names, reads) = AHEAD.with(|ahead| {
        let mut ahead = ahead.borrow_mut();
        let ahead = ahead.as_mut().unwrap();
        let reads = ahead.last_reads.take().filter(|(of, _)| *of == key).map(|(_, reads)| reads);
        (ahead.wanted.get(key).cloned(), reads)
    });
    let names = if is_user_code(Some(file)) {
        for import in file.imports() {
            request_file(program, import.uri());
        }
        Some(Wanted::All)
    } else {
        names
    };
    if let Some(names) = names {
        want_from(program, file, &names);
    }
    if let Some(reads) = reads {
        let reads = if is_user_code(Some(file)) { &reads.reads[..] } else { &reads.reads[..reads.outside] };
        want_reads(program, file, reads);
    }
    flush();
}

/// The dotted names a file reads from the names its import statements bind:
/// `np.linalg.norm` as `[np, linalg, norm]`, at most five names long. Those
/// outside function bodies come first, up to `outside`: the check evaluates the
/// body of a function outside the user files only to infer what it returns.
struct ModuleReads {
    reads: Vec<Vec<Atom>>,
    outside: usize,
}

/// The module reads among `nodes`, all the nodes of one parse, in no particular order.
fn module_reads(nodes: &[P<ParseNode>]) -> ModuleReads {
    let mut bound: FxHashSet<Atom> = FxHashSet::default();
    let mut chains: Vec<(Vec<Atom>, P<ParseNode>)> = Vec::new();
    for &node in nodes {
        if let Some(import) = ImportAsNode::cast(node) {
            match import.d().alias.get() {
                Some(alias) => bound.insert(alias.d().value_atom()),
                None => match import.d().module.get().d().name_parts.borrow().first() {
                    Some(first) => bound.insert(first.d().value_atom()),
                    None => false,
                },
            };
        } else if let Some(import) = ImportFromAsNode::cast(node) {
            bound.insert(import.d().alias.get().unwrap_or(import.d().name.get()).d().value_atom());
        } else if let Some(access) = MemberAccessNode::cast(node)
            && node.parent.get().and_then(MemberAccessNode::cast).is_none_or(|outer| outer.d().left_expr.get() != node)
        {
            // The outermost access of a chain; its prefixes are reads too.
            let mut chain = vec![access.d().member.get().d().value_atom()];
            let mut left = access.d().left_expr.get();
            while let Some(inner) = MemberAccessNode::cast(left) {
                chain.push(inner.d().member.get().d().value_atom());
                left = inner.d().left_expr.get();
            }
            if let Some(base) = NameNode::cast(left) {
                chain.push(base.d().value_atom());
                chain.reverse();
                chain.truncate(5);
                chains.push((chain, node));
            }
        }
    }
    let in_body = |mut node: P<ParseNode>| {
        while let Some(parent) = node.parent.get() {
            if FunctionNode::cast(parent).is_some_and(|function| function.d().suite.get().0 == node) {
                return true;
            }
            node = parent;
        }
        false
    };
    let (mut outside, mut inside): (FxHashSet<Vec<Atom>>, FxHashSet<Vec<Atom>>) = Default::default();
    for (chain, node) in chains {
        if bound.contains(&chain[0]) {
            let reads = if in_body(node) { &mut inside } else { &mut outside };
            for len in 2..=chain.len() {
                reads.insert(chain[..len].to_vec());
            }
        }
    }
    let inside: Vec<Vec<Atom>> = inside.into_iter().filter(|read| !outside.contains(read)).collect();
    let mut reads: Vec<Vec<Atom>> = outside.into_iter().collect();
    let outside = reads.len();
    reads.extend(inside);
    ModuleReads { reads, outside }
}

/// Requests the modules whose names `reads`, from `file`, look up: `m.x` wants `x`
/// from the module `import m` binds, or `import p.m as m`, or `from p import m`.
fn want_reads(program: &Program, file: P<SourceFileInfo>, reads: &[Vec<Atom>]) {
    if reads.is_empty() {
        return;
    }
    let Some(output) = file.source_file.get_parser_output() else { return };
    let imports = file.source_file.get_imports();
    let Some(implicit) = imports.len().checked_sub(output.imported_modules.len()) else { return };
    // The module each name an import statement binds stands for, and the module
    // of each dotted name `import a.b.c` binds.
    let mut aliases: FxHashMap<Atom, Uri> = FxHashMap::default();
    let mut dotted: FxHashMap<Vec<Atom>, Uri> = FxHashMap::default();
    for (module, result) in output.imported_modules.iter().zip(&imports[implicit..]) {
        let target = result.resolved_uris.last().copied().filter(|uri| !uri.is_empty());
        let Some(statement) = module.name_node.parent.get() else { continue };
        if let Some(import) = ImportAsNode::cast(statement) {
            let Some(target) = target else { continue };
            match import.d().alias.get() {
                Some(alias) => {
                    aliases.insert(alias.d().value_atom(), target);
                }
                None => {
                    dotted.insert(module.name_parts.iter().map(|part| Atom::new(part)).collect(), target);
                }
            }
        } else if let Some(import) = ImportFromNode::cast(statement)
            && let Some(submodules) = &result.filtered_implicit_imports
        {
            for name in import.d().imports.borrow().iter() {
                if let Some(submodule) = submodules.get(name.d().name.get().d().value()) {
                    aliases.insert(name.d().alias.get().unwrap_or(name.d().name.get()).d().value_atom(), submodule.uri);
                }
            }
        }
    }
    for read in reads {
        let hit = match aliases.get(&read[0]) {
            Some(&target) => Some((target, read[1])),
            None => (1..read.len()).rev().find_map(|len| dotted.get(&read[..len]).map(|&target| (target, read[len]))),
        };
        let Some((target, name)) = hit else { continue };
        request_file(program, target);
        let new = AHEAD.with(|ahead| {
            ahead.borrow_mut().as_mut().and_then(|ahead| {
                want(&mut ahead.wanted, target.key(), Wanted::Names(std::iter::once(name.as_str().to_string()).collect()))
            })
        });
        if let Some(new) = new
            && let Some(info) = program.get_source_file_info(target)
            && !info.source_file.is_parse_required()
        {
            want_from(program, info, &new);
        }
    }
}

fn request_file(program: &Program, uri: Uri) {
    if let Some(info) = program.get_source_file_info(uri)
        && let Some(request) = info.source_file.parse_ahead_request(&program.config_options())
    {
        self::request(uri.key(), request, true);
    }
}

/// Requests what the import statements of `file`, parsed, bring in for `names`.
fn want_from(program: &Program, file: P<SourceFileInfo>, names: &Wanted) {
    let Some(output) = file.source_file.get_parser_output() else { return };
    let imports = file.source_file.get_imports();
    // The import results start with the implicit ones, such as builtins.
    let Some(implicit) = imports.len().checked_sub(output.imported_modules.len()) else { return };
    for (module, result) in output.imported_modules.iter().zip(&imports[implicit..]) {
        let target = result.resolved_uris.last().copied().filter(|uri| !uri.is_empty());
        let hit = match (&module.imported_symbols, names) {
            // `import a.b`: which attributes the check reads is unknown.
            (None, Wanted::All) => {
                if let Some(target) = target {
                    request_file(program, target);
                }
                continue;
            }
            (None, Wanted::Names(_)) => continue,
            // `from a import *`
            (Some(symbols), names) if symbols.is_empty() => names.clone(),
            (Some(symbols), Wanted::All) => Wanted::Names(symbols.iter().cloned().collect()),
            (Some(symbols), Wanted::Names(names)) => {
                let both: FxHashSet<String> = symbols.iter().filter(|s| names.contains(*s)).cloned().collect();
                if both.is_empty() {
                    continue;
                }
                Wanted::Names(both)
            }
        };
        if let Some(target) = target {
            request_file(program, target);
        }
        if let (Wanted::Names(hit), Some(implicit_imports)) = (&hit, &result.filtered_implicit_imports) {
            for name in hit {
                if let Some(submodule) = implicit_imports.get(name) {
                    request_file(program, submodule.uri);
                }
            }
        }
        let Some(target) = target else { continue };
        let new = AHEAD.with(|ahead| ahead.borrow_mut().as_mut().and_then(|ahead| want(&mut ahead.wanted, target.key(), hit)));
        // A module parsed already passes the new names on now; one not parsed yet
        // passes them on when it is.
        if let Some(new) = new
            && let Some(info) = program.get_source_file_info(target)
            && !info.source_file.is_parse_required()
        {
            want_from(program, info, &new);
        }
    }
}

/// Adds `names` to the names wanted from the module `key` names; returns those it lacked.
fn want(wanted: &mut FxHashMap<&'static str, Wanted>, key: &'static str, names: Wanted) -> Option<Wanted> {
    let entry = wanted.entry(key).or_insert_with(|| Wanted::Names(FxHashSet::default()));
    match (entry, names) {
        (Wanted::All, _) => None,
        (entry, Wanted::All) => {
            *entry = Wanted::All;
            Some(Wanted::All)
        }
        (Wanted::Names(have), Wanted::Names(names)) => {
            let new: FxHashSet<String> = names.into_iter().filter(|name| !have.contains(name)).collect();
            if new.is_empty() {
                return None;
            }
            have.extend(new.iter().cloned());
            Some(Wanted::Names(new))
        }
    }
}

/// Reads the file at `path` as `SourceFile.getFileContent` reads it: `None` when
/// `stat_sync` or the read fails or the file is too large, which the main thread
/// then reports itself.
fn read(path: &str) -> Option<String> {
    let stat = std::fs::metadata(path).ok()?;
    if stat.len() > MAX_SOURCE_FILE_SIZE {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// `SourceFile.parse` from the read through the parse, for a file read from `path`,
/// on a worker: the file's nodes get one block of ids.
fn read_and_parse(path: &str, options: &ParseOptions) -> Option<(Parsed, ModuleReads)> {
    let diag_sink = P::new(DiagnosticSink::new(None));
    let contents = read(path)?;
    let content_hash = get_string_fingerprint(&contents);
    let content_length = contents.js_len();
    let mut nodes = Vec::new();
    let results = with_node_id_block(
        || Parser::new().parse_source_file(JsString::from(contents.as_str()), options.clone(), diag_sink, Some(content_hash)),
        &mut nodes,
    );
    Some((Parsed { contents, content_hash, content_length, diag_sink, results }, module_reads(&nodes)))
}

/// A file parse on the main thread while workers run: its nodes get one block of
/// ids, and `after_parse` learns its module reads.
pub fn parse_here<R>(key: &'static str, parse: impl FnOnce() -> R) -> R {
    let mut nodes = Vec::new();
    let results = with_node_id_block(parse, &mut nodes);
    let reads = module_reads(&nodes);
    AHEAD.with(|ahead| {
        if let Some(ahead) = ahead.borrow_mut().as_mut() {
            ahead.last_reads = Some((key, reads));
        }
    });
    results
}

/// How often an idle worker checks whether the main thread freed memory it allocated.
const IDLE_COLLECT: std::time::Duration = std::time::Duration::from_millis(50);

/// Returns the memory other threads freed into this thread's malloc heap to the OS.
/// mimalloc reclaims a block another thread freed only when the thread that allocated
/// it allocates or collects, so an idle worker would hold the file texts and token
/// buffers of every result the main thread took.
fn collect_freed_memory() {
    #[cfg(not(miri))]
    {
        unsafe extern "C" {
            fn mi_collect(force: bool);
        }
        // SAFETY: `mi_collect` only walks this thread's own mimalloc heap.
        unsafe { mi_collect(true) };
    }
}

fn work(shared: &Shared) {
    jsrt::heap::forbid_collected_allocation();
    let mut collected = 0;
    loop {
        let job = {
            let mut queue = lock(&shared.queue);
            loop {
                if let Some(job) = queue.urgent.pop_front().or_else(|| queue.later.pop_front()) {
                    break job;
                }
                let taken = shared.taken.load(Relaxed);
                if taken != collected {
                    collected = taken;
                    drop(queue);
                    collect_freed_memory();
                    queue = lock(&shared.queue);
                    continue;
                }
                queue = shared.work.wait_timeout(queue, IDLE_COLLECT).unwrap_or_else(PoisonError::into_inner).0;
            }
        };
        {
            let mut state = lock(&job.state);
            if !matches!(*state, State::Queued) {
                continue;
            }
            *state = State::Running;
        }
        let start = Instant::now();
        let before = jsrt::arena::arena_allocated_bytes();
        // A parse that panics leaves the file to the main thread, which then
        // panics as it would have without workers.
        let outcome = match std::panic::catch_unwind(|| read_and_parse(&job.request.path, &job.request.options)) {
            Ok(Some((parsed, reads))) => Outcome::Parsed(Handoff(Box::new(parsed), reads)),
            Ok(None) => Outcome::Unread,
            Err(_) => Outcome::Panicked,
        };
        shared.arena_bytes.fetch_add(jsrt::arena::arena_allocated_bytes() - before, Relaxed);
        shared.busy_ns.fetch_add(start.elapsed().as_nanos() as u64, Relaxed);
        *lock(&job.state) = State::Done(outcome);
        job.done.notify_all();
    }
}

/// `PYRIGHT_RS_PARSE_AHEAD_LOG=1`: prints what the workers did, to stderr.
pub fn log_end_of_check() {
    if std::env::var_os("PYRIGHT_RS_PARSE_AHEAD_LOG").is_none() {
        return;
    }
    // User and system CPU seconds of a thread, from its `stat` file.
    let cpu = |stat: &str| -> f64 {
        let fields: Vec<&str> = stat.rsplit_once(')').map_or("", |(_, rest)| rest).split_whitespace().collect();
        let ticks: f64 = fields.get(11..13).map_or(0.0, |t| t.iter().filter_map(|n| n.parse::<f64>().ok()).sum());
        ticks / 100.0
    };
    let this = std::fs::read_to_string("/proc/thread-self/stat").map_or(0.0, |stat| cpu(&stat));
    let mut workers = 0.0;
    for task in std::fs::read_dir("/proc/self/task").into_iter().flatten().flatten() {
        let comm = std::fs::read_to_string(task.path().join("comm")).unwrap_or_default();
        if comm.starts_with("parse-ahead-") {
            workers += std::fs::read_to_string(task.path().join("stat")).map_or(0.0, |stat| cpu(&stat));
        }
    }
    eprintln!("parse ahead: CPU {this:.2} s on this thread, {workers:.2} s on the workers");
    AHEAD.with(|ahead| {
        let ahead = ahead.borrow();
        let Some(ahead) = ahead.as_ref() else {
            eprintln!("parse ahead: off");
            return;
        };
        let s = &ahead.stats;
        let mut untaken = [0u64; 3];
        for job in ahead.jobs.values() {
            match *lock(&job.state) {
                State::Queued => untaken[0] += 1,
                State::Running => untaken[1] += 1,
                State::Done(_) => untaken[2] += 1,
                State::Taken => {}
            }
        }
        eprintln!(
            "parse ahead: {} workers busy {:.3} s, arenas {} MiB; {} requested; taken {} ready, {} after waiting {:.3} s; \
             parsed on the main thread {} queued, {} unrequested, {} unread, {} panicked, {} stale; untaken {} queued, {} running, {} done",
            thread_count(),
            ahead.shared.busy_ns.load(Relaxed) as f64 / 1e9,
            ahead.shared.arena_bytes.load(Relaxed) >> 20,
            s.requested,
            s.ready,
            s.waited,
            s.wait_ns as f64 / 1e9,
            s.stolen,
            s.unrequested,
            s.unread,
            s.panicked,
            s.stale,
            untaken[0],
            untaken[1],
            untaken[2],
        );
    });
}

/// Check mode: panics unless the worker's parse equals `main`'s, made on this thread.
pub fn check(path: &str, worker: &Parsed, main: &Parsed) {
    let fail = |what: &str| -> ! { panic!("PYRIGHT_RS_PARSE_AHEAD=check: {path}: the worker's {what} differs") };
    if worker.contents != main.contents || worker.content_length != main.content_length {
        fail("file contents");
    }
    if worker.content_hash != main.content_hash || worker.results.content_hash != main.results.content_hash {
        fail("content hash");
    }
    if worker.results.text != main.results.text {
        fail("text");
    }
    if dump_tokenizer_output(&worker.results.tokenizer_output) != dump_tokenizer_output(&main.results.tokenizer_output) {
        fail("tokenizer output");
    }
    if dump_parser_output(&worker.results.parser_output) != dump_parser_output(&main.results.parser_output) {
        fail("parser output");
    }
    let diagnostics = |parsed: &Parsed| -> Vec<String> {
        parsed.diag_sink.peek().iter().map(|diag| format!("{:?}", diag.get())).collect()
    };
    if diagnostics(worker) != diagnostics(main) {
        fail("diagnostics");
    }
    // Ids relative to the module node's: each parse numbers its nodes from one base.
    let ids = |parsed: &Parsed| -> Vec<u32> {
        let root = parsed.results.parser_output.parse_tree;
        let base = root.id.get();
        walk_tree(root.0).iter().map(|node| node.id.get().wrapping_sub(base)).collect()
    };
    if ids(worker) != ids(main) {
        fail("node ids");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Error recovery, a dummy class for decorators, an augmented assignment's copy
    /// of its target, string annotations, an or-pattern with a missing name (whose
    /// message uses an addendum) and an f-string.
    const SOURCE: &str = "import os\nfrom . import x\nx: 'List[int]' = 1\nx += 2\n@dec\n\
        def f(a: \"int\") -> 'str': return f'{a!r:>{w}}'\n\
        match v:\n    case (1 as a) | 2: pass\n@orphan\nclass C(: pass\n";

    fn write(name: &str, text: &str) -> String {
        let path = std::env::temp_dir().join(format!("pyright-rs-parse-ahead-{}-{name}.py", std::process::id()));
        std::fs::write(&path, text).unwrap();
        path.to_str().unwrap().to_string()
    }

    fn parse_on_a_worker(path: &str) -> Parsed {
        let path = path.to_string();
        let Handoff(parsed, _) = std::thread::spawn(move || {
            jsrt::heap::forbid_collected_allocation();
            let (parsed, reads) = read_and_parse(&path, &ParseOptions::new()).unwrap();
            Handoff(Box::new(parsed), reads)
        })
        .join()
        .unwrap();
        *parsed
    }

    #[test]
    fn a_worker_parses_as_the_main_thread_does() {
        let path = write("same", SOURCE);
        let worker = parse_on_a_worker(&path);
        let main = read_and_parse(&path, &ParseOptions::new()).unwrap().0;
        check(&path, &worker, &main);
        assert!(!worker.diag_sink.peek().is_empty());
        // Each parse holds one block of ids, which the other's does not overlap.
        let range = |parsed: &Parsed| {
            let ids: Vec<u32> = walk_tree(parsed.results.parser_output.parse_tree.0).iter().map(|n| n.id.get()).collect();
            (*ids.iter().min().unwrap(), *ids.iter().max().unwrap())
        };
        let (a, b) = (range(&worker), range(&main));
        assert!(a.1 < b.0 || b.1 < a.0, "{a:?} and {b:?} overlap");
        assert_eq!(a.0, worker.results.parser_output.parse_tree.id.get());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn check_panics_on_a_different_parse() {
        let (one, other) = (write("one", SOURCE), write("other", &SOURCE.replace("x += 2", "x -= 2")));
        let worker = parse_on_a_worker(&one);
        let main = read_and_parse(&other, &ParseOptions::new()).unwrap().0;
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&one, &worker, &main))).is_err());
        std::fs::remove_file(one).unwrap();
        std::fs::remove_file(other).unwrap();
    }

    #[test]
    fn an_unreadable_file_is_left_to_the_main_thread() {
        assert!(read_and_parse("/nonexistent/pyright-rs-parse-ahead.py", &ParseOptions::new()).is_none());
    }
}
