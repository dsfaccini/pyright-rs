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

## order of evaluation

- Keep every call in the order the TypeScript makes it. Keep every loop in the order it iterates.
- Keep early returns, short circuits and side effects where they are. Pyright's results depend on evaluation order.
- Do not restructure, deduplicate or optimize while porting. A faster version is a cache: `AGENTS.md` covers caches.

## object references

- A TypeScript object that is shared, stored in more than one place, or compared with `===` becomes a `jsrt::P<T>`.
- `P<T>` is a Copy handle into a leak arena. `==` on `P<T>` is identity, like `===` on objects.
- Compare values by content only where the TypeScript compares by content.
- A field that any TypeScript code writes after construction becomes `Cell<T>` for Copy types or `RefCell<T>` otherwise.
- A field computed once on first use becomes `OnceCell<T>`.
- Never hold a `RefCell` borrow across a call that can reach back into the same object. Copy the value out first.

## memory

pyright allocates about 23 times what it keeps alive: 194 GB over a pydantic-ai check, 8.4 GB live (`docs/research/memory-advice.md`). These rules keep a later collector of evaluator objects possible.

- Allocate with `P::new_eval` in evaluator and checker code. Keep `P::new` for parsing, binding, configuration and the program.
- Make an object a `P` only when pyright compares it with `===`, or writes to it after storing it elsewhere. Otherwise use an owned value, or `Rc` when it is shared and acyclic. The arena never runs Drop.
- Never store a `&'static T` taken from `P::get` in a struct. Store the `P`.
- Never capture a `P` in a closure that a struct stores. Write the stored callback as a struct with named fields and a method, so a collector can see every `P`.
- Reach parse-node and type details only through typed handles and `d()`. Create them only through their `create` functions. Their memory layout will change.
- Set `PYRIGHT_RS_ALLOC_STATS=1` to print objects and bytes per type and heap when `pyright-rs` exits.

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
- Use `jsrt::JsString` (UTF-16) for unescaped Python string-literal values. Escapes such as `\ud800` give them lone surrogates.
- Where JavaScript and Rust string operations differ, call the `jsrt` helper: length, `slice`, `substring`, `substr`, `charCodeAt`, `<` and `>`, `trim`, `toLowerCase`, `toUpperCase`, `split`, `indexOf`, `normalize('NFKC')`, `JSON.stringify` and `String.prototype.replace`.
- Known gap: a `JsString` with a lone surrogate prints as U+FFFD inside a `String` message. Count such a case as a port failure when the oracle finds one.

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

- `ParseNode` is one struct: `node_type`, `start`, `length`, `id`, `parent`, `a`, and `d`.
- `d` is `ParseNodeDetails`, an enum with one variant per `ParseNodeType`. Each variant holds a `<Name>NodeDetails` struct.
- Every `d` field is `Cell<T>`, or `RefCell<Vec<T>>` for arrays. Every node field written after creation is a `Cell`. The parser writes most fields after `create`.
- A node handle is `P<ParseNode>`. Each node type also gets a typed handle, for example `FunctionNode(P<ParseNode>)`, with `cast(P<ParseNode>) -> Option<Self>`, `d()` returning its details, and `Deref` to `ParseNode`.
- A TypeScript union of node types, such as `ExpressionNode`, is a type alias for `P<ParseNode>`.
- Each `create` function keeps its parent assignments and range extensions in TypeScript order.

## exceptions and the stack

- Port a `try`/`catch` only where pyright throws on purpose and expects the catch to run.
- A catch that only turns a pyright crash into a diagnostic is not ported. A Rust panic is a port bug.
- The CLI runs the check on a thread with a 1 GiB stack. Rust must never overflow where JavaScript did not.

## dependencies

- Add a crate with `cargo add`, never by editing `Cargo.toml`.
- Add a crate only when it matches the JavaScript behavior that pyright observes. Record why in your notes file.

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
