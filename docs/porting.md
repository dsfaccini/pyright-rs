# Porting contract

How TypeScript in `~/pyright/packages/pyright-internal/src` becomes Rust in `crates/pyright/src`.
Every porting agent follows this file. Change a rule here first, then in the code.

## layout

- `crates/pyright` holds the port. Its module tree mirrors `packages/pyright-internal/src`.
- Name each Rust file after its TypeScript file in snake_case: `parser/tokenizerTypes.ts` becomes `parser/tokenizer_types.rs`.
- `crates/pyright/src/oracle/` holds the Rust side of the dumps described in `oracle/FORMAT.md`. It ports no pyright file.
- `crates/jsrt` holds what TypeScript got from JavaScript and V8: object references, ordered collections, string, number and sort semantics. It ports no pyright file.
- Put Rust-only helpers in `jsrt`, never in a ported file.

## markers

- Put `// <path>.ts:<line>` on the line above each ported function, method, struct and enum.
- `<path>` is relative to `packages/pyright-internal/src`, for example `// parser/tokenizer.ts:412`.
- `<line>` is the line of the TypeScript declaration at the pinned commit.

## names

- Types, enums and enum variants keep their TypeScript names.
- Functions, methods, fields and locals become snake_case. Keep a leading underscore: `_readIndent` becomes `_read_indent`.
- Module-level constants become SCREAMING_SNAKE_CASE.
- A Rust keyword gets a trailing underscore: `type` becomes `type_`.
- A method named `clone` gets a trailing underscore: `clone` becomes `clone_`. On a `P<T>`, `.clone()` resolves to `P`'s `Clone` and copies the handle, as assigning a JS reference does; it never reaches a method `T::clone`.

## order of evaluation

- Keep every call in the order the TypeScript makes it. Keep every loop in the order it iterates.
- Keep early returns, short circuits and side effects where they are. Pyright's results depend on evaluation order.
- Do not restructure, deduplicate or optimize while porting. A faster version is a cache: `AGENTS.md` covers caches.

## object references

- A TypeScript object that is shared, stored in more than one place, or compared with `===` becomes a `jsrt::P<T>`.
- `P<T>` is a Copy handle to an object on the program or the evaluator heap ("memory" below). `==` on `P<T>` is identity, like `===` on objects.
- Compare values by content only where the TypeScript compares by content.
- A field that any TypeScript code writes after construction becomes `Cell<T>` for Copy types or `RefCell<T>` otherwise.
- A field computed once on first use becomes `OnceCell<T>`.
- Never hold a `RefCell` borrow across a call that can reach back into the same object. Copy the value out first.

## memory

pyright allocates about 23 times what it keeps alive: 194 GB over a pydantic-ai check, 8.4 GB live (`docs/research/memory-advice.md`). A collector reclaims evaluator objects between files (`docs/collector-design.md`). These rules keep it correct.

- Allocate with `P::new_eval` in evaluator and checker code. Keep `P::new` for parsing, binding, configuration and the program.
- Make an object a `P` only when pyright compares it with `===`, or writes to it after storing it elsewhere. Otherwise use an owned value, or `Rc` when it is shared and acyclic. A program object is never dropped; a swept evaluator object is dropped in place during a collection, so never implement `Drop` for a type that dereferences a `P` or allocates one.
- Derive `jsrt::Trace` on every struct and enum that a heap object holds. Add `#[trace(eval)]` to a type allocated with `P::new_eval`; `P::new_eval` does not compile without it.
- Mark a field `#[trace(program)]` when its type allows an evaluator handle but it only ever holds program handles, such as a declaration only the binder sets. Check mode verifies it.
- Use `jsrt::cell::{Cell, RefCell, OnceCell}`, not `std::cell`, for interior mutability: their writes run the generational collector's write barrier. A std cell that can hold an evaluator handle does not compile. A std cell is fine for state no heap object holds (thread-locals, CLI options, test harnesses).
- Keep every cell that can hold an evaluator handle in its heap object's own bytes, or behind another `P`: never inside a `Vec`, `Box`, `Rc` or map that the object owns. The barrier finds the written object by the cell's address; `P::new_eval` and `P::new` do not compile for a type that breaks this (`Trace::CELL_OUT_OF_LINE`). `jsrt::OptionRefCell` boxes its storage and runs the barrier itself, so its value may hold handles but no cell.
- An object that lives for the whole run, such as a singleton type, goes on the program heap (`P::new`).
- A thread-local that holds an evaluator handle registers a root with `jsrt::heap::add_root` in its initializer (`ENUM_EVAL_STACK`).
- Never store a `&'static T` taken from `P::get` in a struct. Store the `P`.
- Never capture a `P` in a closure that a struct stores. Write the stored callback as a struct with named fields and a method, so the collector can see every `P`. A stored closure that captures program handles only is allowed: mark its field `#[trace(skip)]` and name what it captures.
- Reach parse-node and type details only through typed handles and `d()`. Create them only through their `create` functions. Their memory layout will change.
- Give each header type a `Cell` field, or a zero-sized `UnsafeCell<()>` field when it has no `Cell`. A header type is a `#[repr(C)]` struct allocated with details behind it: `ParseNode`, `Token`, `FlowNode`, `Declaration`. Code writes those details through a `&` to the header. Without interior mutability in the header, release builds treat that reference as read-only memory and drop the writes.
- Keep evaluator objects small: pyright-rs creates millions of them. Box a record or map that few objects set (`Option<Box<T>>`), or move such fields into the struct's cold half (`[cold]` in `tools/stubgen/typemap.toml`; read with `cold()`, write with `cold_mut()`). Lock the size with `const _: () = assert!(size_of::<T>() <= n);` after the type.
- Tokens live in their tokenizer output's scratch region, and the output frees the tokens no parse node holds when it drops, at the end of `SourceFile.parse`, as pyright drops its `tokenizerOutput`. Names, strings and f-string parts, which parse nodes nearly always hold, go to the program heap directly.
- The collector's switches are under "switches" below.

## switches

Every Rust-only environment variable is listed here. When you add a cache or shortcut that pyright lacks, add its switch to this list (`AGENTS.md`, "gates").

| variable | values | effect |
|---|---|---|
| `PYRIGHT_RS_MODULE_PATH` | a directory | holds `typeshed-fallback`; default: the executable's directory |
| `PYRIGHT_RS_GC` | `off`, `on` (default), `check` | collection of the evaluator heap (`docs/collector-design.md`); `check` needs the `jsrt/gc-check` feature |
| `PYRIGHT_RS_GC_LOG` | `1`, `2` | prints each collection; `2` also prints the live and freed objects per type |
| `PYRIGHT_RS_GC_RATIO`, `PYRIGHT_RS_GC_MIN_MIB` | numbers | the collection trigger: generational, a collection every `MIN_MIB` (default 512) of evaluator slots; otherwise also `RATIO` (`MIN_MIB` default 1024), and both 0 collect before every file |
| `PYRIGHT_RS_GC_THREADS` | a number; default the available parallelism, at most 8 | threads that mark, the collecting thread included; 1 marks on the collecting thread alone |
| `PYRIGHT_RS_GC_GENERATIONAL` | `0`, `1` (default) | generational collection: a minor collection traces young objects and the old ones written since the last collection |
| `PYRIGHT_RS_GC_MAJOR_RATIO` | a number; default 1 | generational: a collection is major once the old objects grew by this share since the last major one |
| `PYRIGHT_RS_GC_WARM_MIB` | a number; default 256 | MiB of released evaluator blocks kept resident for reuse |
| `PYRIGHT_RS_GC_CENSUS` | `1` | prints a breakdown of the resident set after each mark, at the end of the check and at exit; changes no result; buffer bytes need the `pyright` crate's `count-malloc` feature (`docs/collector-design.md`, "census") |
| `PYRIGHT_RS_NODE_INFO_STORE_CACHE` | `on` (default), `off`, `check` | `AnalyzerNodeInfoContextImpl::_store_for` reuses the store of its last lookup |
| `PYRIGHT_RS_SCOPE_ID_CACHE` | `on` (default), `off`, `check` | `get_scope_id_for_node` remembers each node's scope id |
| `PYRIGHT_RS_TOKENS` | `free` (default), `keep`, `check` | `keep` puts every token on the program heap; `check` makes a freed tokenizer scratch region inaccessible, so a later use faults |
| `PYRIGHT_RS_ALLOC_STATS` | `1`, or a number n | prints objects and bytes per type and heap at exit; with n, also each time the evaluator heap grows by another n GiB |

## collections

- `Map` becomes `jsrt::JsMap`. `Set` becomes `jsrt::JsSet`. Both iterate in insertion order, like JavaScript.
- Delete with `shift_remove`. `swap_remove` changes the iteration order.
- Use `FxHashMap` and `FxHashSet` only for lookups. Never iterate them.
- An object literal used as a dictionary iterates integer-like keys first, ascending, then the rest in insertion order. Port that order where the TypeScript iterates such an object.
- `Array` becomes `Vec`.

## strings

- Source text is `[u16]`, UTF-16 code units, as JavaScript holds it. The tokenizer reads it like `charCodeAt`.
- Every text position and length is a UTF-16 offset. Never use a UTF-8 byte offset for a value pyright can observe.
- Use `String` and `&str` for identifiers, messages, paths and other text. Source text always decodes to valid Unicode, so identifiers are valid `str`.
- A string field that every clone of a hot evaluator object copies is `Rc<str>`: immutable and shared, as a JS string is (`TypeVarDetailsPriv` scope names, `FunctionParam.name`; `[sites]` in `tools/stubgen/typemap.toml`).
- A key string that pyright hashes or compares with `===` on a hot path is a `jsrt::Atom`, interned once per thread: two atoms of a thread are equal exactly when their texts are, so hashing and comparing one is integer work. Take `atom!("text")` for a literal, `Atom::new` for built text, and `Atom::lookup` to probe a table without interning. Where pyright sorts, compares with `<` or `>`, or prints the string, use `as_str()`. An atom changes how the text is held, not what any operation returns, so it needs no switch. Interned: TypeVar scope ids (`TypeVarScopeId`) names with scope (`TypeVarDetailsPriv.name_with_scope`, the constraint-set keys), class names (`ClassDetailsShared.name` and `full_name`, the alias name, the names `ClassType::is_built_in` takes), and symbol names (`SymbolTable` keys, identifier token values, `NameNode::value_atom`). A `SymbolTable` lookup takes an atom or text (`jsrt::AtomKey`); text this thread never interned is in no table. Code-flow reference keys (`create_key_for_reference`, the code-flow expression sets, the flow-node type cache keys) are atoms too.
- Use `jsrt::JsString` (UTF-16) for unescaped Python string-literal values. Escapes such as `\ud800` give them lone surrogates.
- Where JavaScript and Rust string operations differ, call the `jsrt` helper: length, `slice`, `substring`, `substr`, `charCodeAt`, `<` and `>`, `trim`, `toLowerCase`, `toUpperCase`, `split`, `indexOf`, `normalize('NFKC')`, `JSON.stringify` and `String.prototype.replace`.
- Known gap: a `JsString` with a lone surrogate becomes U+FFFD inside a `String`: in a message, and in a `String` key, where two names that differ only in a lone surrogate collide. Count such a case as a port failure when the oracle finds one. Two key sites, keyed by text because `SymbolTable` and TypedDict entries are keyed by text throughout the port:
  - named tuple field names, `create_named_tuple_type` (`analyzer/named_tuples.rs`): `NamedTuple('X', [('a\ud800', int), ('a\udc00', str)])` reports "Names within a named tuple must be unique", which pyright does not;
  - functional TypedDict entry names (`analyzer/typed_dicts.rs`): `TypedDict('T', {'a\ud800': int, 'a\udc00': str})` reports "Names within a dictionary must be unique" and merges the two entries, so `t: T = {'a\ud800': 1, 'a\udc00': 's'}` fails to assign. pyright reports neither.

## numbers

- Text offsets, lengths, lines and columns are `i32`.
- Ids and counters are `u32`.
- A TypeScript `number` that holds a computed or fractional value is `f64`.
- A Python number literal is `NumberValue`: an `f64` for a JS number, a big integer for a JS `bigint`. The tokenizer chooses which, as `tokenizer.ts` does.
- Convert to `usize` only to index a `Vec`. Format a number for output with the `jsrt` helper that matches JavaScript `String(n)`.

## enums and flags

- A `const enum` or `enum` becomes a `#[repr(i32)]` Rust enum with the same variants, in the same order, with the same values.
- An enum whose members are bit flags (`1 << n`) becomes a newtype over `u32`, with one associated constant per member and the bit operators.

## types, classes and unions

- An `interface` or `class` becomes a `struct`. Its methods go in an `impl` block in the same order.
- `T | undefined` and optional members become `Option<T>`.
- A union discriminated by a kind field (`nodeType`, `type`, `category`) becomes a struct with the kind field plus an enum of per-kind detail structs. Parse nodes below are the pattern.
- A TypeScript interface implemented by several classes becomes a trait only when the TypeScript calls through the interface.

## parse nodes

- `ParseNode` is the header every node shares: `node_type`, `start`, `length`, `id`, `parent` and `a`. Each node type has a `<Name>NodeDetails` struct.
- A node is allocated as its header followed by its own details, so it takes the size of its type, not of the largest type. `node.d()` returns `ParseNodeDetails`, an enum with one variant per `ParseNodeType`, each holding a `&'static <Name>NodeDetails`.
- Every `d` field is `Cell<T>`, or `RefCell<Vec<T>>` for arrays. Every node field written after creation is a `Cell`. The parser writes most fields after `create`.
- A node handle is `P<ParseNode>`. Each node type also gets a typed handle, for example `FunctionNode(P<ParseNode>)`, with `cast(P<ParseNode>) -> Option<Self>`, `d()` returning its details, and `Deref` to `ParseNode`.
- A TypeScript union of node types, such as `ExpressionNode`, is a type alias for `P<ParseNode>`.
- Each `create` function keeps its parent assignments and range extensions in TypeScript order.
- A node field that holds a token is a `TokenCell`: storing a token there copies it out of its tokenizer's scratch region (`Token::keep`). Nothing else may hold a token past its `TokenizerOutput`.

## exceptions and the stack

- Port a `try`/`catch` only where pyright throws on purpose and expects the catch to run.
- A catch that only turns a pyright crash into a diagnostic is not ported. A Rust panic is a port bug.
- The CLI runs the check on a thread with a 1 GiB stack. Rust must never overflow where JavaScript did not.

## dependencies

- Add a crate with `cargo add`, never by editing `Cargo.toml`.
- Add a crate only when it matches the JavaScript behavior that pyright observes. Record why in your notes file.
- The build needs a C compiler: `pyright-rs` uses mimalloc as its global allocator (`crates/pyright/src/main.rs`), which `libmimalloc-sys` compiles from C.

## tests

- Port pyright's own unit tests for each ported file when they exist, for example `tests/tokenizer.test.ts` to Rust tests in the same module.
- The oracle comparison in `oracle/` is the conformance test. A stage counts as ported when its dumps match on the whole corpus.

## agent workflow

- Work in your own worktree, `~/pyright-rs-wt/<name>`, on branch `<name>`, created from `main`.
- Edit only the files the main session assigned to you.
- Keep `notes/<name>.md` with four sections: Signature changes, Shared-file edits, Needs from others, Doubts.
- Record every edit to a file you do not own under Shared-file edits.
- Commit often. Before you report a deliverable, rebase on `main` and make `cargo build`, `cargo test` and `cargo clippy --all-targets` pass without warnings.
- The main session merges branches into `main`. Never commit to `main`.
- Commit a piece that is ready for `main` with a subject that starts with `READY:`. The main session merges `READY:` commits as they land.
- Never rewrite a commit after the main session merged it. Rebase on `main` instead.
