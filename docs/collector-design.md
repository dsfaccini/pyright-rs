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

- Two address ranges, reserved once per process with `mmap(MAP_NORESERVE)`: 256 GiB for the evaluator heap, 64 GiB for typed program blocks. Pages are committed on first touch. "Is this handle in the evaluator heap" is one subtraction and one compare.
- Each range is cut into 64 KiB blocks. A block belongs to one thread and holds objects of one Rust type, in fixed slots of `size_of` rounded up to the alignment (at least 8 bytes, so every `P` has its own address).
- Block metadata lives in a side table indexed by block number: the type, the slot size, a bump index, a free list, and bitmaps for live and marked slots (plus dead slots in check mode). No per-object header: `Type` keeps its `size_of`.
- Every `P::new_eval` goes to a typed block. A `P::new` goes to a typed block when its type has `HOLDS_EVAL` (below), because the collector scans those objects as roots; every other program object stays in the bump arena, which parse and bind speed depends on. The choice is made at compile time from the const.
- The allocator id of a non-generic type sits in a static the derive emits (`jsrt::ClassSlot`), so finding the type's blocks is one load. A generic type allocated directly (`RefCell<Vec<P<Type>>>`) falls back to a per-thread `TypeId` map.
- `libc` provides `mmap` and `madvise`. Code: `crates/jsrt/src/heap.rs`, `trace.rs`, `arena.rs`; the derive in `crates/jsrt_derive`.

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
- `#[trace(eval)]` on a type sets `EVAL`. `P::new_eval` does not compile for a type without it, so a `P<T>` whose `T` lacks `EVAL` never points into the evaluator heap.
- A std or dependency type has `EVAL = HOLDS_EVAL`: the evaluator allocates `RefCell<Vec<P<Type>>>` directly, and a `P<RefCell<JsSet<String>>>` (the binder's code-flow expressions) stays a program handle. A list of variances holds no handle, so `TypeAliasSharedInfo.computed_variance` is allocated with `P::new`.
- Field attributes:
  - `#[trace(skip)]`: an opaque value, such as a stored closure or a `&'static dyn` service.
  - `#[trace(program)]`: the field's type allows an evaluator handle, but the field only ever holds program handles (`AnalyzerNodeInfo.declaration`: the binder's declarations). Check mode verifies it.
- `#[trace(holds_eval)]` on a type forces `HOLDS_EVAL`, for a type that contains itself without a `P` (the const would otherwise be a cycle): `TypeResultWithNode`.
- `jsrt` implements `Trace` for the std and dependency types the port stores: `Option`, `Vec`, `Box`, `Rc`, `Cell<T: Copy>`, `RefCell`, `OnceCell`, `JsMap`, `JsSet`, `FxHashMap`, `FxHashSet`, tuples, arrays, strings, numbers, `fn` pointers and `&'static T` (holds no evaluator handle; check mode verifies it).
- `RefCell` contents are read with `try_borrow_unguarded`. A `RefCell` mutably borrowed at a collection panics.

## roots

- **The program heap.** It is never collected, so every program object is a root. The collector scans every typed program block, that is every program object whose type has `HOLDS_EVAL`. `ParseNode`, `Token`, `AnalyzerNodeInfo`, `FlowNode`, `Declaration` and `Scope` hold no evaluator handle and stay in the bump arena. `AnalyzerNodeInfo.declaration` and `Declaration.submodule_fallback` are `#[trace(program)]`: only the binder sets them. `Program` and `Symbol` are scanned; the current `TypeEvaluator` and its caches (`typeCache`, `speculativeTypeTracker`, `codeFlowAnalyzerCache`, ...) are reached through `Program.evaluator`.
- **Thread-locals.** A thread-local that holds an evaluator handle registers a root with `jsrt::heap::add_root(fn(&mut Tracer))` in its initializer: `ENUM_EVAL_STACK` (`enums.rs`), `PROTOCOL_ASSIGNMENT_STACK` (`protocols.rs`). Both are empty between files; tracing them costs nothing and does not depend on that.
- **Permanent objects** go on the program heap: the singleton instances in `types.rs` (of `UnboundType`, `UnknownType`, `NeverType` and `AnyType`) move from `P::new_eval` to `P::new`.
- **The stack.** At the collection point the frames above `Program.analyze` hold program handles only (`pyright.rs`, `service.rs`, `background_analysis_program.rs`, `analysis.rs`).
- **Stored closures** are opaque (`#[trace(skip)]`). Each may capture program handles only: `ImportLookup` captures the `Program`, `ImportLookupResult.doc_string` a module node, the `getModuleName` callback of each `SourceFile` the `Program`; file-system filters and cleanups capture no heap handle; `Scope.chained_module_level_scope_lookup` is set only for notebook cells. Type-narrowing callbacks capture evaluator handles but are only passed and returned, never stored.
- **Trait objects.** `P<dyn AnalyzerNodeInfoReader>` is a program handle (`EVAL = false`); `P<dyn FunctionReturnTypeCallback>` can be the evaluator (`EVAL = true`).

## sweep

- A live, unmarked evaluator slot is dropped in place. `drop_in_place` frees the `Vec`, `String` and map buffers it owns, and releases its `Rc`s. The slot then joins its block's free list.
- No port type implements `Drop`, and the std drops never dereference a `P`, so the order in which dead objects drop does not matter.
- A block left with no live slot returns to the process's free-block pool, and `madvise(MADV_DONTNEED)` returns its pages to the OS.

## trigger

- Before each file, collect when the evaluator slot bytes allocated since the last collection reach `max(live bytes after the last collection, 1 GiB)`. The evaluator heap then stays under twice the live set plus one file.
- `PYRIGHT_RS_GC_MIN_MIB` overrides the 1 GiB floor. Check mode collects on the floor alone, 64 MiB by default; `PYRIGHT_RS_GC_MIN_MIB=0` collects before every file that follows an allocation.
- `PYRIGHT_RS_GC_LOG=1` prints each collection: live bytes before and after, objects freed, milliseconds.

## switch and check mode

`PYRIGHT_RS_GC` selects the mode, read once per process:

- `off`: no collection. Allocation still goes through the typed blocks.
- `on`: collect as above.
- `check`: needs a build with `--features jsrt/gc-check` (`CHECK_FEATURES=jsrt/gc-check oracle/corpus/check.sh`). Collect as `on`, and:
  - a swept slot is dropped, filled with a poison byte, marked dead, and never reused; its block is never released;
  - dereferencing (`Deref`, `P::get`), comparing (`==`, `P::ptr_eq`) or hashing a `P` to a dead slot panics with the type name;
  - marking a handle to a free or dead slot panics;
  - every program object goes to a typed block, and the root scan visits every field of every one, and panics when an evaluator handle sits in a `#[trace(program)]` field, behind a `&'static`, or in a `P<T>` whose `T` lacks `EVAL`;
  - collections run on the floor alone (above), so they run often.

The `P` hooks are compiled only into a `gc-check` build: as a runtime flag, the load on every dereference cost about 5% wall time on the samples corpus (1.88 s against 1.80 s without it, collection off).

## rollout

1. Machinery with `PYRIGHT_RS_GC=off` as the default: allocator, `Trace`, derive, roots, check mode, `docs/porting.md` rules. `main` keeps passing. Gate: with collection off, samples wall time and `oracle_bind --timing` parse and bind time on pydantic-ai within noise of `main`. Compare builds with `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` too: with the default 16 codegen units, adding the `Trace` impls alone moved parse and bind time by 5% (it reshuffles which functions can inline into each other), and with one unit the difference was under 1%.
2. Check mode on the seven corpora, output identical.
3. `on` becomes the default once step 2 passes; pydantic-ai runs to completion under the 30G cap, output identical to the reference.

## rules for ported code

`docs/porting.md`, "memory", holds them.

## rejected

- **Conservative scanning** (Boehm-style). Handles also sit in `Vec` and map buffers on the system heap, so it needs a replacement global allocator that can find allocation bounds, plus stack scanning. The precise traces are what `docs/porting.md` "memory" was written for.
- **Reference counting.** Types form cycles (a class and its methods, recursive aliases), and `P` would stop being `Copy`.
- **Per-file regions.** Evaluator caches keep types from earlier files alive.
- **Moving or compacting** (Immix's opportunistic evacuation). It breaks `P` identity and `Copy`.
- **Per-object headers.** A vtable word per object adds 25% to `ConstraintTracker` and 12% to `ConstraintSolutionSet`; typed blocks keep the type in the block.

## sources

- Immix, mark-region collection with line and block marking: https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf
- Bacon, Cheng and Rajan, "A Unified Theory of Garbage Collection" (tracing as the dual of reference counting): https://dl.acm.org/doi/10.1145/1028976.1028982
