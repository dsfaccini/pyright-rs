# Evaluator heap collector

A non-moving mark-sweep collector for the evaluator heap (`P::new_eval`), run between files in the check loop.
Recommendation 1 in `docs/research/memory-advice.md` gives the motivation; this file gives the design.

## why

- pyright-rs on pydantic-ai grows its evaluator heap by about 0.75 GB/s and is killed at 30 GB, 1,190 files into the check. pyright peaks at 9,073,044 KB maximum RSS (`~/pyright-rs-data/reference/pydantic-ai/gate-nodump.time.txt`).
- Evaluator slot bytes allocated per checked file, measured at 27af4b8: at most 0.76 GB (`tests/test_agent.py`), then 0.69 GB (`models/openai.py`); every other file under 0.36 GB.
- So a collection before each file bounds the heap by the live set plus one file.

## shape

- **When.** Before each file in `Program.analyze`'s loop over the source file list (`program.ts:766`). No evaluator or checker frame is live there, so no stack scanning.
- **Precise.** Every type a heap object holds implements `jsrt::Trace`, through `#[derive(jsrt::Trace)]`. The collector visits only typed fields, never raw words.
- **Non-moving.** A live object keeps its address. `P` stays `Copy`; `P::ptr_eq`, `==` and `Hash` keep their meaning.
- **Stop-the-world, single thread.** Each thread has its own heaps, as now. The CLI checks on one thread.

## heap layout

- Two address ranges, reserved once per process with `mmap(MAP_NORESERVE)`: 256 GiB for the evaluator heap, 64 GiB for the program heap. Pages are committed on first touch. "Is this handle in the evaluator heap" is one subtraction and one compare.
- Each range is cut into 64 KiB blocks. A block belongs to one thread and holds objects of one Rust type, in fixed slots of `size_of` rounded up to the alignment (at least 8 bytes, so every `P` has its own address).
- Block metadata lives in a side table indexed by block number: the type, the slot size, a bump index, a free list, and bitmaps for live and marked slots (plus dead slots in check mode). No per-object header: `Type` stays 584 bytes.
- `P::new` and `P::new_eval` find the type's allocator through a per-thread table keyed by `TypeId`, then pop the block's free list or bump.
- The program heap uses the same blocks and is never swept. It replaces bumpalo because the collector enumerates program objects by type (below), which a bump arena cannot do.
- `libc` provides `mmap` and `madvise`.

## tracing

```rust
pub unsafe trait Trace {
    const HOLDS_EVAL: bool; // a value of this type can hold an evaluator-heap handle
    const EVAL: bool;       // a value of this type can live on the evaluator heap
    fn trace(&self, t: &mut Tracer);
}
```

- `P<T>` has `HOLDS_EVAL = T::EVAL`. Its `trace` marks the target when the address is in the evaluator range. Marking sets the slot's mark bit and pushes the object on a mark stack; the object's type comes from its block, so a `P<dyn Trait>` traces like any other.
- `#[derive(jsrt::Trace)]` (new proc-macro crate `crates/jsrt_derive`, on the `syn` and `quote` already in `Cargo.lock`): `HOLDS_EVAL` is the OR over the fields; `trace` visits the fields whose type has `HOLDS_EVAL`.
- `#[trace(eval)]` on a type sets `EVAL`. `P::new_eval` does not compile for a type without it. Generic containers (`RefCell<T>`, `Vec<T>`, maps) count as `EVAL`, since the evaluator allocates `RefCell<Vec<P<Type>>>` directly.
- Field attributes:
  - `#[trace(skip)]`: an opaque value, such as a stored closure or a `&'static dyn` service.
  - `#[trace(program)]`: the field's type allows an evaluator handle, but the field only ever holds program handles (`AnalyzerNodeInfo.declaration`: the binder's declarations). Check mode verifies it.
- `#[trace(holds_eval)]` on a type forces `HOLDS_EVAL`, for a type that contains itself without a `P` (the const would otherwise be a cycle).
- `jsrt` implements `Trace` for the std and dependency types the port stores: `Option`, `Vec`, `Box`, `Rc`, `Cell<T: Copy>`, `RefCell`, `OnceCell`, `JsMap`, `JsSet`, `FxHashMap`, `FxHashSet`, tuples, arrays, strings, numbers, `fn` pointers and `&'static T` (holds no evaluator handle; check mode verifies it).
- `RefCell` contents are read with `try_borrow_unguarded`. A `RefCell` mutably borrowed at a collection panics.

## roots

- **The program heap.** It is never collected, so every program object is a root. The collector scans every program block whose type has `HOLDS_EVAL`. `ParseNode`, `Token`, `AnalyzerNodeInfo` and `FlowNode` (about 30 M objects on pydantic-ai) hold no evaluator handle, so their blocks are skipped. `Program`, `Symbol`, `Declaration` and `SourceFile` are scanned; the current `TypeEvaluator` and its caches (`typeCache`, `speculativeTypeTracker`, `codeFlowAnalyzerCache`, ...) are reached through `Program.evaluator`.
- **Thread-locals.** A thread-local that holds an evaluator handle registers a root with `jsrt::arena::add_root(fn(&mut Tracer))` in its initializer: `ENUM_EVAL_STACK` (`enums.rs`), `PROTOCOL_ASSIGNMENT_STACK` (`protocols.rs`). Both are empty between files; tracing them costs nothing and does not depend on that.
- **Permanent objects** go on the program heap: the singleton instances in `types.rs` (of `UnboundType`, `UnknownType`, `NeverType` and `AnyType`) move from `P::new_eval` to `P::new`.
- **The stack.** At the collection point the frames above `Program.analyze` hold program handles only (`pyright.rs`, `service.rs`, `background_analysis_program.rs`, `analysis.rs`).
- **Stored closures** are opaque (`#[trace(skip)]`). Each may capture program handles only: `ImportLookup` captures the `Program`, `ImportLookupResult.doc_string` a module node, the `getModuleName` callback of each `SourceFile` the `Program`. Type-narrowing callbacks capture evaluator handles but never outlive the call that made them.

## sweep

- A live, unmarked evaluator slot is dropped in place. `drop_in_place` frees the `Vec`, `String` and map buffers it owns, and releases its `Rc`s. The slot then joins its block's free list.
- No port type implements `Drop`, and the std drops never dereference a `P`, so the order in which dead objects drop does not matter.
- A block left with no live slot returns to the process's free-block pool, and `madvise(MADV_DONTNEED)` returns its pages to the OS.

## trigger

- Before each file, collect when the evaluator slot bytes allocated since the last collection reach `max(live bytes after the last collection, 1 GiB)`. The evaluator heap then stays under twice the live set plus one file.
- `PYRIGHT_RS_GC_MIN_MIB` overrides the 1 GiB floor.
- `PYRIGHT_RS_GC_LOG=1` prints each collection: live bytes before and after, objects freed, milliseconds.

## switch and check mode

`PYRIGHT_RS_GC` selects the mode, read once per process:

- `off`: no collection. Allocation still goes through the typed blocks.
- `on`: collect as above.
- `check`: collect as `on`, and:
  - a swept slot is dropped, filled with a poison byte, marked dead, and never reused; its block is never released;
  - dereferencing (`Deref`, `P::get`), comparing (`==`, `P::ptr_eq`) or hashing a `P` to a dead slot panics with the type name;
  - marking a handle to a free or dead slot panics;
  - the root scan visits every program block and every field, and panics when an evaluator handle sits in a type without `HOLDS_EVAL`, in a `#[trace(program)]` field, or behind a `&'static`;
  - the floor drops to 64 MiB, so collections run often.

The `P` hooks cost a load of a process-wide flag on every dereference. If that costs more than 1% wall time on pydantic-ai with collection off, check mode becomes a cargo feature instead of a runtime mode.

## rollout

1. Machinery with `PYRIGHT_RS_GC=off` as the default: allocator, `Trace`, derive, roots, check mode, `docs/porting.md` rules. `main` keeps passing.
2. Check mode on the seven corpora, output identical.
3. `on` becomes the default once step 2 passes; pydantic-ai runs to completion under the 30G cap, output identical to the reference.

## rules for ported code

These go into `docs/porting.md`, "memory", with step 1:

- Derive `jsrt::Trace` on every struct and enum that a heap object holds. Add `#[trace(eval)]` to a type allocated with `P::new_eval`.
- A stored closure captures program handles only. Mark its field `#[trace(skip)]` and name what it captures.
- A thread-local that holds an evaluator handle registers a root.
- An object that lives for the whole run goes on the program heap.

## rejected

- **Conservative scanning** (Boehm-style). Handles also sit in `Vec` and map buffers on the system heap, so it needs a replacement global allocator that can find allocation bounds, plus stack scanning. The precise traces are what `docs/porting.md` "memory" was written for.
- **Reference counting.** Types form cycles (a class and its methods, recursive aliases), and `P` would stop being `Copy`.
- **Per-file regions.** Evaluator caches keep types from earlier files alive.
- **Moving or compacting** (Immix's opportunistic evacuation). It breaks `P` identity and `Copy`.
- **Per-object headers.** A vtable word per object adds 25% to `ConstraintTracker` and 12% to `ConstraintSolutionSet`; typed blocks keep the type in the block.

## sources

- Immix, mark-region collection with line and block marking: https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf
- Bacon, Cheng and Rajan, "A Unified Theory of Garbage Collection" (tracing as the dual of reference counting): https://dl.acm.org/doi/10.1145/1028976.1028982
