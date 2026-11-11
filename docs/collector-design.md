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
- **Stop-the-world.** Each thread has its own heaps, as now. The CLI checks on one thread. Marking runs on that thread and on parked helper threads while it waits (below); sweeping runs on that thread alone.

## heap layout

- Two address ranges, reserved once per process with `mmap(PROT_NONE, MAP_NORESERVE)`: 256 GiB for the evaluator heap, 64 GiB for typed program blocks. Under an address-space limit (`ulimit -v`) they shrink to a quarter and a sixteenth of it, and a refused reservation is retried at half the size. Blocks become readable and writable 512 at a time (`mprotect`), so strict overcommit charges only what is handed out; pages are backed on first touch. "Is this handle in the evaluator heap" is one subtraction and one compare.
- Each range is cut into 64 KiB blocks. A block belongs to one thread and holds objects of one Rust type, in fixed slots of `size_of` rounded up to the alignment (at least 8 bytes, so every `P` has its own address).
- Block metadata lives in a side table indexed by block number: the type, the slot size, a bump index (program blocks), and bitmaps for live and marked slots (plus dead slots in check mode). No per-object header: `Type` keeps its `size_of`.
- An evaluator class allocates from a cursor over its current block's live bitmap: the free slots of one 64-slot word, taken lowest first. Allocation never reads the slot it hands out, which after a sweep is no longer in cache; a free list threaded through the slots would cost a cache miss per allocation.
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
- Objects leave the mark stack through a FIFO of eight, prefetched when they enter it, so the cache misses of several objects overlap (Cher, Hosking and Vijaykumar, "Software Prefetching for Mark-Sweep Garbage Collection"). The test for an already marked object comes first; the checks for a dangling handle run only on objects marked for the first time.
- `#[derive(jsrt::Trace)]` (new proc-macro crate `crates/jsrt_derive`, on the `syn` and `quote` already in `Cargo.lock`): `HOLDS_EVAL` is the OR over the fields; `trace` visits the fields whose type has `HOLDS_EVAL`.
- `#[trace(eval)]` on a type sets `EVAL`. `P::new_eval` does not compile for a type without it, so a `P<T>` whose `T` lacks `EVAL` never points into the evaluator heap.
- A std or dependency type has `EVAL = HOLDS_EVAL`: the evaluator allocates `RefCell<Vec<P<Type>>>` directly, and a `P<RefCell<JsSet<String>>>` (the binder's code-flow expressions) stays a program handle. A list of variances holds no handle, so `TypeAliasSharedInfo.computed_variance` is allocated with `P::new`.
- Field attributes:
  - `#[trace(skip)]`: an opaque value, such as a stored closure or a `&'static dyn` service.
  - `#[trace(program)]`: the field's type allows an evaluator handle, but the field only ever holds program handles (`AnalyzerNodeInfo.declaration`: the binder's declarations). Check mode verifies it.
- `#[trace(holds_eval)]` on a type forces `HOLDS_EVAL`, for a type that contains itself without a `P` (the const would otherwise be a cycle): `TypeResultWithNode`.
- `jsrt` implements `Trace` for the std and dependency types the port stores: `Option`, `Vec`, `Box`, `Rc`, `Cell<T: Copy>`, `RefCell`, `OnceCell`, `JsMap`, `JsSet`, `FxHashMap`, `FxHashSet`, tuples, arrays, strings, numbers, `fn` pointers and `&'static T` (holds no evaluator handle; check mode verifies it).
- `RefCell` contents are read with `try_borrow_unguarded`. A `RefCell` mutably borrowed at a collection panics.
- `P::to_dyn` to a trait needs `U::EVAL` when `T::EVAL`, so an evaluator object is never held as an untraced trait object.

## roots

- **The program heap.** It is never collected, so every program object is a root. The collector scans every typed program block, that is every program object whose type has `HOLDS_EVAL`. `ParseNode`, `Token`, `AnalyzerNodeInfo`, `FlowNode`, `Declaration` and `Scope` hold no evaluator handle and stay in the bump arena. `AnalyzerNodeInfo.declaration` and `Declaration.submodule_fallback` are `#[trace(program)]`: only the binder sets them. `Program` and `Symbol` are scanned; the current `TypeEvaluator` and its caches (`typeCache`, `speculativeTypeTracker`, `codeFlowAnalyzerCache`, ...) are reached through `Program.evaluator`.
- **Thread-locals.** A thread-local that holds an evaluator handle registers a root with `jsrt::heap::add_root(fn(&mut Tracer))` in its initializer: `ENUM_EVAL_STACK` (`enums.rs`), `PROTOCOL_ASSIGNMENT_STACK` (`protocols.rs`). Both are empty between files; tracing them costs nothing and does not depend on that.
- **Permanent objects** go on the program heap: the singleton instances in `types.rs` (of `UnboundType`, `UnknownType`, `NeverType` and `AnyType`) move from `P::new_eval` to `P::new`.
- **The stack.** At the collection point the frames above `Program.analyze` hold program handles only (`pyright.rs`, `service.rs`, `background_analysis_program.rs`, `analysis.rs`).
- **Stored closures** are opaque (`#[trace(skip)]`). Each may capture program handles only: `ImportLookup` captures the `Program`, `ImportLookupResult.doc_string` a module node, the `getModuleName` callback of each `SourceFile` the `Program`; file-system filters and cleanups capture no heap handle; `Scope.chained_module_level_scope_lookup` is set only for notebook cells. Type-narrowing callbacks capture evaluator handles but are only passed and returned, never stored.
- **Trait objects.** `P<dyn AnalyzerNodeInfoReader>` is a program handle (`EVAL = false`); `P<dyn FunctionReturnTypeCallback>` can be the evaluator (`EVAL = true`).
- **Other threads.** `P` is neither `Send` nor `Sync`: a block belongs to one thread, and only that thread's collector frees it.

## parallel marking

- `PYRIGHT_RS_GC_THREADS` threads mark, the collecting thread included: by default the available parallelism, at most 4. At 1 the collecting thread marks alone.
- The collecting thread traces the registered roots first, since they are its thread-locals. Then every thread takes program blocks to scan from a shared counter, and traces from its own mark stack.
- A thread whose stack holds at least two packets of 512 entries hands its oldest packet to a shared pool while some thread is idle. A thread with no work takes a packet; the mark ends when every thread is idle and the pool is empty.
- Mark bits are set with an atomic `fetch_or`, so only one thread pushes an object. Tracing only reads objects: `RefCell` contents through `try_borrow_unguarded`, `Cell` through `get`, `Rc` without touching its counts. The mutator is stopped, so nothing writes while helpers read.
- Helper threads are started on the first parallel mark and parked on a condition variable between collections. A panic on any thread (a borrowed `RefCell`, a check-mode assertion) stops the others and unwinds on the collecting thread, which clears the marks as for a serial mark.
- The set of marked objects is reachability, so it does not depend on the thread count; the sweep visits blocks in the same order as before.

## write barrier

- `jsrt::cell::{Cell, RefCell, OnceCell}` wrap the std cells with the same API. Before a write that can store an evaluator handle (`Cell::set`, `replace`, `take`, `swap`; `RefCell::borrow_mut`, `try_borrow_mut`, `replace`, `replace_with`, `take`, `swap`; `OnceCell::set`, `get_or_init` on an empty cell), they call `jsrt::heap::remember` with the cell's address. A cell whose value type has `HOLDS_EVAL` false skips the call at compile time.
- `remember` marks a card: one byte per 512 bytes of each reserved range, in a card table reserved next to the block metadata and committed with the blocks. An address outside the typed blocks (the stack, a buffer) marks nothing. With generational collection off, the barrier's range lengths stay 0 and it marks nothing.
- A mutable borrow marks the card when it starts; no collection runs while a borrow is live, so writes through the `RefMut` are covered.
- The barrier finds the written object by the cell's address, so no heap object may keep such a cell in a buffer it owns. `Trace` carries two consts the derive computes: `HAS_CELL` (holds a cell that can hold an evaluator handle) and `CELL_OUT_OF_LINE` (such a cell sits behind a `Vec`, `Box`, `Rc`, `VecDeque` or map). `P::new_eval` and `P::new` assert `!CELL_OUT_OF_LINE` at compile time. `OptionRefCell` boxes its `RefCell` but runs the barrier on its own address, so it counts as in line. A `#[trace(holds_eval)]` type claims no cells (the const would be a cycle): it must hold none.
- The std cells' `Trace` impls assert at compile time that their value holds no evaluator handle, so a std cell in a heap object is a compile error.

## generational collection

`PYRIGHT_RS_GC_GENERATIONAL=1` turns it on.

- **Sticky marks.** The sweep leaves every survivor marked. A marked object is old; an object allocated since the last collection is unmarked, so young.
- **Minor collection.** Traces from the registered roots, the program objects allocated since the last collection (each program block keeps the slots a collection traced), and the old program and evaluator objects on marked cards. Marking stops at old objects, which are already marked. The sweep drops only unmarked objects, so old objects survive, garbage or not.
- **Major collection.** Clears every mark bit and traces from every root, as without generations.
- **When.** A collection is due every `PYRIGHT_RS_GC_MIN_MIB` of evaluator slots. It is major when the live slot bytes reach `(1 + PYRIGHT_RS_GC_MAJOR_RATIO) ×` those after the last major collection (default ratio 1), and the first time. A mark interrupted by a panic leaves marks that no longer tell old objects apart, so the next collection is major.
- Every collection ends with clean cards and every program object old.
- Old garbage waits for a major collection; a young object that only old garbage reaches survives until then too.

## sweep

- A live, unmarked evaluator slot is dropped in place. `drop_in_place` frees the `Vec`, `String` and map buffers it owns, and releases its `Rc`s. Its live bit clears, so the allocation cursor hands the slot out again.
- Dropping misses the cache on the buffers a dead object owns. `Trace::prefetch_owned`, which the derive writes like `trace`, prefetches a value's `Vec`, `String`, `Box`, `Rc` and `IndexMap` buffers; the sweep calls it on the dead object four slots ahead of the one it drops.
- No port type implements `Drop`, and the std drops never dereference a `P`, so the order in which dead objects drop does not matter.
- Allocation, root registration and collection never nest: the heap panics when a root, `Trace` impl or drop allocates.
- A panic while marking (check mode's assertions, a borrowed `RefCell`) clears the marks before it unwinds, so a later collection traces everything again. A panic while sweeping aborts the process.
- A block left with no live slot returns to the process's free-block pool. Up to `PYRIGHT_RS_GC_WARM_MIB` of such blocks (default 256) keep their pages, and new blocks come from them first, so allocating into them does not fault; `madvise(MADV_DONTNEED)` returns the pages of the rest to the OS.

## trigger

- Before each file, collect when the evaluator slot bytes allocated since the last collection reach `max(ratio × live slot bytes after the last collection, floor)`. Defaults: ratio 1 and floor 1 GiB; check mode ratio 0 and floor 64 MiB. `PYRIGHT_RS_GC_RATIO` and `PYRIGHT_RS_GC_MIN_MIB` override them; both at 0 collect before every file that follows an allocation.
- The trigger counts slots only. The `Vec`, `String` and map buffers that dead objects own come on top: on pydantic-ai each collection frees 1.1 to 1.6 GB of them next to about 1 GB of slots.
- `PYRIGHT_RS_GC_LOG=1` prints each collection: the time since start, milliseconds (and those spent marking), program objects scanned, objects freed, live slot bytes, slot bytes allocated since the last collection, evaluator and typed program block bytes, program arena bytes and chunk bytes, the bytes the collecting thread holds in the global allocator before and after (only with the `pyright` crate's `count-malloc` feature), and the resident set before and after with the share of each reserved range. `PYRIGHT_RS_GC_LOG=2` also prints the evaluator handles traced and a table of live and freed objects per type.
- The global allocator bytes come from `jsrt::heap::CountingAlloc`, which wraps the `pyright-rs` binary's mimalloc under `count-malloc` and counts per thread. Counting every allocation costs about 0.6% of the instructions on pydantic-ai, so the default build does not count.

## switch and check mode

`PYRIGHT_RS_GC` selects the mode, read once per process:

- `off`: no collection. Allocation still goes through the typed blocks.
- `on`, the default: collect as above.
- `check`: needs a build with `--features jsrt/gc-check` (`CHECK_FEATURES=jsrt/gc-check oracle/corpus/check.sh`). Collect as `on`, and:
  - a swept slot is dropped, filled with a poison byte, marked dead, and never reused; its block is never released;
  - dereferencing (`Deref`, `P::get`), comparing (`==`, `P::ptr_eq`) or hashing a `P` to a dead slot panics with the type name;
  - marking a handle to a free or dead slot panics;
  - every program object goes to a typed block, and the root scan visits every field of every one, and panics when an evaluator handle sits in a `#[trace(program)]` field, behind a `&'static`, or in a `P<T>` whose `T` lacks `EVAL`;
  - collections run on the floor alone (above), so they run often;
  - a parallel mark is followed by a serial one, and the collection panics when their mark bits differ;
  - a minor collection's mark is followed by a full one from every root, and the collection panics on an object the full mark reaches but the minor one missed: a write the barrier did not see. The minor mark is then restored, so old garbage stays as in `on`.

The `P` hooks are compiled only into a `gc-check` build: as a runtime flag, the load on every dereference cost about 5% wall time on the samples corpus (1.88 s against 1.80 s without it, collection off).

## rollout

1. Machinery with `PYRIGHT_RS_GC=off` as the default: allocator, `Trace`, derive, roots, check mode, `docs/porting.md` rules. `main` keeps passing. Gate: with collection off, samples wall time and `oracle_bind --timing` parse and bind time on pydantic-ai within noise of `main`. With the default 16 codegen units, adding the `Trace` impls alone moved parse and bind time by 5% (it reshuffles which functions can inline into each other); with one unit the difference was under 1%, and release builds use one unit since 51c777d.
2. Check mode on the seven corpora, output identical.
3. `on` becomes the default once step 2 passes, and once the seven corpora are identical with `PYRIGHT_RS_GC=on PYRIGHT_RS_GC_MIN_MIB=0 PYRIGHT_RS_GC_RATIO=0` (slot reuse and block release, which check mode never exercises); pydantic-ai runs to completion under the 30G cap, output identical to the reference.

## rules for ported code

`docs/porting.md`, "memory", holds them.

## rejected

- **Conservative scanning** (Boehm-style). Handles also sit in `Vec` and map buffers on the system heap, so it needs a replacement global allocator that can find allocation bounds, plus stack scanning. The precise traces are what `docs/porting.md` "memory" was written for.
- **Reference counting.** Types form cycles (a class and its methods, recursive aliases), and `P` would stop being `Copy`.
- **Per-file regions.** Evaluator caches keep types from earlier files alive.
- **Moving or compacting** (Immix's opportunistic evacuation). It breaks `P` identity and `Copy`.
- **Per-object headers.** A vtable word per object adds 25% to `ConstraintTracker` and 12% to `ConstraintSolutionSet`; typed blocks keep the type in the block.

## sources

- Cher, Hosking and Vijaykumar, "Software Prefetching for Mark-Sweep Garbage Collection: Hardware Analysis and Software Redesign" (ASPLOS 2004), the prefetch FIFO: https://hosking.github.io/links/Cher+2004ASPLOS.pdf
- Immix, mark-region collection with line and block marking: https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf
- Bacon, Cheng and Rajan, "A Unified Theory of Garbage Collection" (tracing as the dual of reference counting): https://dl.acm.org/doi/10.1145/1028976.1028982
