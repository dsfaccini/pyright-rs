# Speed and memory levers, ranked

Research from 2026-11-11, measured at `main` e3dedfc on devbox (Ryzen 7 7700, 8 cores, 62 GB; other agents ran alongside, load average 5 to 7).
It ranks the architectural levers for a faster and leaner pyright-rs that keeps pyright 1.1.414's output.
Hotspot fixes, compiler settings, collector tuning and program-heap layout belong to the `hot`, `build`, `gc` and `progmem` agents; each lever names where it overlaps them.

Every measurement file named here is in `~/pyright-rs-data/bench/explore/`, with the scripts that produced it.
`explore-prototypes.patch` holds the prototypes behind the measurements. None of them is meant for `main`.

## where the time goes

Baseline, pydantic-ai, `bench/measure.sh` at b110b3b: 388.7 G instructions, 52.3 s wall, 11.8 GB peak RSS.

Samples of a frame-pointer build, `perf record -F 999 --call-graph fp`, 56,322 samples over a 57.5 s run (`callgraph-profile.txt`). Each sample counts once, under its leaf-most phase frame:

| phase | share |
|---|---|
| collector (`ThreadHeap::collect`) | 13.4% |
| tokenize, parse, bind, import resolution, file reads | 7.8% (1.4, 2.1, 2.9, 1.2, 0.2) |
| checker and evaluator | 75.4% |
| stack cut off at 127 frames, no phase frame | 3.4% |

By leaf frame, outside the collector:
- `malloc`, `free`, `realloc` and `Vec` growth: 18.4%.
- kernel: 5.6%. Mostly page faults on first touch under `P::new_eval` and mimalloc page extension, plus `madvise`.
- hash tables (`hashbrown`, `indexmap`): 5.1%.
- string work in the four leaf-most frames: 10.3% (formatting 4.8%, string hashing 3.1%, `String` clones 2.1%, UTF-16 helpers 1.0%).

Phase timers, `--stats` (`stats-timers.txt`, 58.4 s run): Read 0.31 s, Tokenize 0.87 s, Parse 1.24 s, Resolve Imports 0.72 s, Bind 2.36 s, Check 45.05 s.
Collector log of the same run (`gc-log.txt`): 6 collections, 8.41 s, of which marking 5.18 s.

`perf stat -M PipelineL1` (`perf-stat-topdown-pydantic-ai.txt`): 24.4% of dispatch slots retiring, 29.3% frontend-bound, 42.3% backend-bound, 3.8% bad speculation; 1.46 instructions per cycle; 53.1 s user, 3.2 s system, 1.22 M page faults.
On samples (`perf-stat-topdown-samples.txt`) the frontend-bound share is 41.7%. The binary holds 6.7 MB of code.

## ranking

Bounds are shares of the pydantic-ai run unless stated. Costs are estimates.

| rank | lever | measured upper bound | output risk | cost (estimate) | overlaps |
|---|---|---|---|---|---|
| 1 | collector: parallel marking, then generational marking | marking 5.18 s, sweep 3.23 s | none; soundness only | 1–2 weeks parallel, 3–4 weeks generational | `gc` |
| 2 | fewer system-heap allocations | 18.4% of samples, plus frees in the sweep; 1.0 G allocations | low | 3–6 weeks, spread over the evaluator | `hot`, `progmem` |
| 3 | parallel tokenize and parse ahead of the checker | 2.11 s of timers; 5.5x on 8 threads | ids and thread-local singletons | 2–3 weeks; bind 3 more | — |
| 4 | interned names and keys | 10.3% of samples; about 5% left after `hot`'s branch | low | 2–4 weeks | `hot`, `progmem` |
| 5 | keep released pages committed | 5.6% of samples in the kernel; 3.2 s system | none | days | `gc` |
| 6 | dense node-id side tables | type cache: 0.70% of instructions (prototype); the family: about 4% | none | 1 week | `hot` |
| 7 | 32-bit handles | memory: at most 0.89 GB of 10.98 GB resident | none | 3–4 weeks | `progmem` |
| 8 | lazy diagnostic text | about 3% | high | 2 weeks | `hot` |
| 9 | import resolution and file-system caching | 1.56% of samples, 0.25% in syscalls | low | days | — |
| — | code layout (PGO, BOLT) | 29.3% of slots frontend-bound | none | `build`'s lane | `build` |

Top three: collector marking, allocation count, parallel front end.

## 1. collector marking

**Bound.** Marking takes 5.18 s and sweeping 3.23 s (`gc-log.txt`). Each collection marks the whole live set again: 162 MiB of live slots after the first, 1,305 MiB after the sixth, and 0.3 M to 1.2 M program objects scanned as roots each time.

**Parallel marking.** The mutator is stopped while marking, and tracing only reads: `Cell::get`, `RefCell::try_borrow_unguarded`, `Rc` dereference. Mark bits become atomic `fetch_or`, the mark stack becomes per-thread with work stealing, and program root blocks split across threads.
- Saving: most of the 5.18 s of wall time on 8 cores; an estimate, not measured. Instructions do not drop.
- Sweeping stays serial: dropping a dead object decrements `Rc` counts it can share with objects that another sweeping thread drops, and `Rc` counts are not atomic.

**Generational marking.** Keep mark bits across collections ("sticky" marks), trace only young objects, and rescan old objects that were written since the last collection. Linux soft-dirty bits (`/proc/self/clear_refs`, `/proc/self/pagemap` bit 55) give written pages without a write barrier in the port's code. They work unprivileged on devbox.
- Prototype (`soft-dirty-*.txt`): after each collection, clear the soft-dirty bits; at the next one, count old objects on written pages.
- As the heap allocates now, it does not work: new objects fill the free slots next to old ones, so 87–94% of the full marking work remains.
- With new objects allocated only in fresh blocks, the remaining work falls to 64%, 43%, 30%, 26% and 22% at collections 2 to 6. Weighted by each collection's mark time: about 1.7 s of marking instead of 5.0 s (estimate: assumes mark time proportional to objects traced, and leaves out reading the dirty bits).
- Costs: peak RSS rose from 12.34 GB to 15.46 GB in the prototype, which never reuses a freed slot. A real design reuses the holes after a full collection.
- Gap: a pointer written into an old object's buffer on the system heap (a `Vec` behind a `Box`) dirties a malloc page, not the owner's slot. 53–93% of the other anonymous pages were dirty between collections. Such owners need rescanning every time, or a type-level flag from the derive.

**Output risk.** None while the collector is correct: collection is not observable. A missed root is a use after free, which check mode (`PYRIGHT_RS_GC=check`) catches by poisoning swept slots.

**Code.** `crates/jsrt/src/heap.rs` (mark loop, block allocation, mark bits), `crates/jsrt/src/trace.rs`; for generational marking also the derive in `crates/jsrt_derive`.

## 2. fewer system-heap allocations

**Bound.** A counting global allocator (`alloc-count.txt`): 1,001 M allocations and 27 M reallocations per run, 41.3 GB requested.

| size | allocations |
|---|---|
| 1–7 bytes | 143 M |
| 8–15 bytes | 434 M |
| 16–31 bytes | 118 M |
| 32–63 bytes | 210 M |
| 64–255 bytes | 89 M |
| 256 bytes or more | 4 M |

Allocation and free leaf frames take 18.4% of samples outside the collector. Inside it, drop glue and `free` take 2.95% more: the sweep drops each dead object and frees the buffers it owns one by one.
The allocator sees sizes, not sites. Samples in allocation frames fall under `validate_arg_type` (11.2%), `assign_type` (5.4%), the code flow engine (3.7%), `ConstraintSolutionSet` (2.5%), `apply_solved_type_vars` (2.3%), `ConstraintTracker` (1.9%) and `get_name_with_scope` (1.6%) (`callgraph-profile.txt`; an ancestor can count in several rows).

**Options.**
- Share arrays and strings that pyright shares. `TypeBase.cloneType` (`analyzer/types.ts:266`) copies `props` and `priv` one level deep, so a type and its clones share every array in `priv`. An `Rc<[T]>` there reproduces pyright's aliasing; the port's `Vec` clone is a copy pyright never makes. `hot` does this for union subtypes, base classes, MRO and type parameters.
- Inline storage for vectors of zero or one element.
- Evaluator-owned buffers on the collected heap, so the sweep frees them with their owner's slot instead of one `free` each.

**Output risk.** Low: none of these changes what a value holds. Sharing must stop where pyright copies: a shared array may not be mutated through one owner when pyright's owners hold separate arrays.

**Code.** The `[cold]` and `[sites]` tables of `tools/stubgen/typemap.toml`, the type definitions in `analyzer/types.rs`, `constraint_tracker.rs`, `constraint_solution.rs`, `type_evaluator_types.rs`, and their call sites.

## 3. parallel tokenize and parse ahead of the checker

**Bound.** Tokenize and Parse timers: 2.11 s; Read 0.31 s; Bind 2.36 s; Resolve Imports 0.72 s (`stats-timers.txt`). In the profile the front end is 7.8% of samples.
Prototype `par_parse` (`par-parse.txt`): tokenize and parse the 6,696 files of the pydantic-ai reference dump on N threads, each with its own heaps. 3.61 s on 1 thread, 1.89 s on 2, 1.12 s on 4, 0.66 s on 8: 5.5x.
So tokenize and parse ahead of the checker save up to about 1.7 s, 3% of the baseline (estimate from the scaling). Bind adds up to 2.36 s more but has the dependencies below.

**Output risk and obstacles.**
- Global counters, each a thread-local in the port: node ids (`parseNodes.ts:184`, `parse_nodes.rs` `_NEXT_NODE_ID`), symbol ids (`symbol.ts:80`), flow node ids (`codeFlowTypes.ts:61`). No output reads an id's value: a search of `analyzer`, `parser` and `common` for ids in comparisons, sorts and built strings finds only debug logging, and the oracle dumps print none (`oracle/FORMAT.md`). Ids only key maps and must stay unique: per-file id blocks do that. The file id (`sourceFile.ts:1096`) is assigned when the main thread creates the `SourceFile`, which does not move.
- Thread-local singletons held as handles: the parser reaches `EMPTY_STRING_ANNOTATION_INFO` (`parse_nodes.rs`), the binder `UNREACHABLE_STATIC_CONDITION_FLOW_NODE`; others are `Uri` `EMPTY` and the singleton types in `types.rs`. A worker thread makes its own copy of each, with its own identity. Parse output must use the main thread's, or the singletons must become process-wide.
- `P` is neither `Send` nor `Sync`. Parse output lives in the worker's bump arena, which is never freed and holds no evaluator handle, so handing a finished tree to the main thread once is sound behind an `unsafe` wrapper.
- Order. pyright parses a file when the checker first needs it (`Program._parseFile`, `program.ts:2008`) and counts it then (`_parsedFileCount`, `program.ts:2018`, which feeds `getCacheUsage`). Workers may parse ahead, but the main thread must take each result at pyright's point, and parse diagnostics stay with their file.
- Bind is harder: the binder looks up other files through `importLookup` (wildcard imports, `binder.ts:2065`), which binds them on demand (`program.ts:2171`), and every file binds after builtins.
- Waste: workers know which files are needed only after import resolution. The 2,046 user files are certain; third-party files parsed speculatively but never needed cost memory, since the program heap is never freed.

**Code.** `parser/parse_nodes.rs`, `analyzer/symbol.rs`, `analyzer/code_flow_types.rs` (id counters), `analyzer/program.rs` (`_parse_file`), `analyzer/source_file.rs`, the thread-local singletons above, `crates/jsrt/src/arena.rs`.

## 4. interned names and keys

**Bound.** String work is 10.3% of samples outside the collector. Formatting comes mostly from `TypeVarType::get_name_with_scope` (1.6%), `get_scope_id_for_node` (0.7%) and `pop_symbol_resolution` (0.7%); string hashing from `ApplySolvedTypeVarsTransformer` (0.6%), `get_abstract_symbol_info` (0.4%) and `ConstraintSet::clone_` (0.3%).
`hot` merged `is_built_in` (a50b092; 1.52% inclusive) and has work in progress on `get_name_with_scope` (3.46% inclusive), `make_name_with_scope` and `pop_symbol_resolution`'s eager message. These are part of the 10.3%; about 5% remains after them (estimate).

**The keys.** `ConstraintSet._typeVarMap` is keyed by the name-with-scope string (`constraintTracker.ts:42`, built by `makeNameWithScope`, `types.ts:3120`); TypeVar scope ids are strings (`getScopeIdForNode`, `parseTreeUtils.ts:2713`); symbol tables are `Map<string, Symbol>` (`symbol.ts:322`); code flow reference keys are strings (`createKeyForReference`, `codeFlowTypes.ts:223`).
An interned atom (`u32`, one table per process) makes these keys integer compares and hashes, and removes their allocations.

**Output risk.** Low. Maps keep insertion order whatever the key type. Where pyright sorts or prints a name, the atom's text is used. The lone-surrogate gap of `docs/porting.md` "strings" stays as it is.

**Code.** `analyzer/types.rs` (TypeVar names and scope ids), `constraint_tracker.rs`, `symbol.rs` (`SymbolTable`), `code_flow_types.rs`, `parse_tree_utils.rs`, `parser/parse_nodes.rs` (`NameNodeDetails.value`, which `progmem` has work in progress to read from the token).

## 5. keep released pages committed

**Bound.** 5.6% of samples are in the kernel, 3.2 s of system time and 1.22 M page faults (`perf-stat-topdown-pydantic-ai.txt`). Callers: `TypeBase::clone_type` (0.94%), `P::new_eval` of `VirtualParamDetails` and `TypeResult`, mimalloc's `mi_page_free_list_extend` (0.66%), `madvise` (0.42%).
The collector returns a fully dead block with `MADV_DONTNEED` (`heap.rs` `release_block`); the next allocation into it faults again.

**Option.** Keep released blocks committed up to a budget, and reuse them first. This trades RSS for faults. Transparent huge pages are in `madvise` mode on devbox; `MIMALLOC_ALLOW_LARGE_OS_PAGES=1` left the page faults on samples unchanged (56,652 and 56,651, `perf-stat-tlb-samples*.txt`). L2 TLB misses are 51 M on pydantic-ai; at 30 to 100 cycles a page walk that is 0.6–1.9% of the 266 G cycles (estimate), so huge pages save little beyond the faults.

**Output risk.** None. **Code.** `crates/jsrt/src/heap.rs`. **Cost.** Days.

## 6. dense node-id side tables

**Bound.** Prototype: the main `typeCache` as a slab indexed by node id (`TypeCacheMap` in `explore-prototypes.patch`). pydantic-ai 400.82 G to 398.02 G instructions (−0.70%), samples 14.37 G to 14.29 G (−0.61%), `--outputjson` identical (`dense-type-cache.txt`; the prototype binary also counted allocations, hence the higher base).
The same shape fits the other node-keyed tables: `AnalyzerNodeInfoStore` (three hash lookups per read through `AnalyzerNodeInfoContextImpl::_store_for`; 2.91% inclusive), `SpeculativeTypeTracker` (1.30%), the expected-type and TypeForm caches, and the flow-node-keyed code flow caches. Together about 4% (estimate; the inclusive shares overlap). `hot` has work in progress on the node info store and the reachability caches.

**Output risk.** None where the map is never iterated. `typeCache` (`typeEvaluator.ts:676`) is read and written by node id and deleted by `leaveSpeculativeContext` (`typeCacheUtils.ts:122`); its entry count feeds `getCacheUsage` and `_handleMemoryHighUsage` (`program.ts:1148`, `program.ts:1242`), so the slab must keep an exact count. Lever 3 must keep node ids compact for the slab to stay small.

**Code.** `analyzer/type_evaluator.rs`, `type_cache_utils.rs`, `analyzer_node_info.rs`, `code_flow_engine.rs`. Switch and check mode per `AGENTS.md` "gates".

## 7. 32-bit handles

**Bound.** A scan of resident memory at the third and sixth collections (`handle-scan.txt`) counts the 8-byte words that hold a heap handle (an address in the evaluator range, the typed program range or a bump-arena chunk):
- collection 3: 149 M words, 1,136 MiB, 15.3% of 7,437 MiB resident;
- collection 6: 232 M words, 1,770 MiB, 16.1% of 10,978 MiB resident.

Halving them saves at most 0.89 GB at the sixth collection, under 8% of resident memory. The scan counts stale words in freed malloc memory and misses `None` handles, so the bound is approximate.
`ParseNode` (13.3 M objects, 160 bytes) pays for its largest details variant, so narrower handles shrink it little until its details are exact-size, which `progmem` is doing.

**Speed.** tsrs measured 6.5% more instructions for 14–15% less memory (`docs/research/memory-advice.md`). Every dereference adds a base and a shift.

**Output risk.** None: `P`'s identity, equality and hash keep their meaning. **Code.** `crates/jsrt/src/arena.rs` (`P`), `heap.rs` (one reserved window of at most 32 GiB for both heaps with a 3-bit shift), the bump arena moved into that window, `P<dyn Trait>`.

## 8. lazy diagnostic text

**Bound.** `type_printer` 1.34%, `localize` 0.88% and `DiagnosticAddendum` 1.58% of samples, inclusive and overlapping. pydantic-ai reports no diagnostic, so none of this text reaches the output.

**Output risk: high.** Printing a function type calls the return-type callback (`typePrinter.ts:1370`), which the evaluator passes as `getEffectiveReturnType` and which infers the return type: cache writes and incomplete-generation bumps. Deferring the text moves that inference, and pyright's results depend on evaluation order. Only text whose printing provably evaluates nothing can be deferred.

## 9. import resolution and file-system caching

**Bound.** `import_resolver` frames are 1.56% of samples inclusive, 0.25% of them in syscalls; Resolve Imports is 0.72 s. samples makes about 12,000 syscalls in all (`strace-samples.txt`). Regex matching takes 0.29% of samples.
Small. **Output risk.** Low, given invalidation never happens in a CLI run. **Code.** `analyzer/import_resolver.rs`, `import_resolver_file_system.rs`.

## code layout

29.3% of dispatch slots are frontend-bound on pydantic-ai and 41.7% on samples. Profile-guided optimization and BOLT target that, with no output risk. `build`'s lane; no bound measured here.

## rejected

- **Generational marking on dirty pages without separating new objects.** Measured: 87–94% of the marking work remains (`soft-dirty-with-slot-reuse.txt`).
- **Parallel sweeping.** Dead objects drop non-atomic `Rc` counts that other objects share.
- **Parallel checking.** pyright's own `--threads` changes output on typeshed (`docs/research/memory-advice.md`, "rejected").

## how to reproduce

- Profile: build with `RUSTFLAGS="-C force-frame-pointers=yes"`, run `perf record -F 999 --call-graph fp` through `run.sh pydantic-ai <out>`, then `perf script --no-inline -F ip,sym` into `stacks.py`, and `phase.py`, `attr.py`, `allockind.py` on the result.
- `run.sh` runs a recorded corpus under the shared pydantic-ai lock and a 30G memory cap, as `bench/measure.sh` does.
- Prototypes: apply `explore-prototypes.patch` to e3dedfc. Switches: `PYRIGHT_RS_PROTO_DENSE`, `PYRIGHT_RS_PROTO_DIRTY`, `PYRIGHT_RS_PROTO_NOREUSE`, `PYRIGHT_RS_PROTO_SCAN`; the counting allocator is always on in that build; `cargo run --release --example par_parse -- <reference dump> <threads>`.
