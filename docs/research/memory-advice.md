# Memory and speed advice for step 5 and later

Research report from 2026-11-10, written before step 5 started. It answers whether peak memory can stay bounded once the evaluator adds types to a never-freeing arena, and what could make the port faster than about 3x without changing output.

## measurements

pyright 1.1.414 on pydantic-ai, `node --max-old-space-size=16384 --expose-gc --trace-gc-nvp ... --stats --verbose`, no type-cache empty, 2:18 wall, 9.15 GB peak RSS:
- 194.3 GB allocated over the run (V8 `total_allocated_bytes`);
- 8.39 GB live after the last full GC, at 129.6 s of 137 s;
- scavenges promoted 6.8 GB, so about 96% of allocated bytes die young;
- the live set grows steadily: 1.2 GB at 14 s, 3.9 GB at 43 s, 5.9 GB at 77 s, 8.4 GB at 130 s.

pyright-rs `--outputjson` on pydantic-ai at milestone 1 (2,047 files, `PYRIGHT_RS_ALLOC_STATS=1`), program heap only:

| type | objects | bytes |
|---|---|---|
| `ParseNode` | 5,501,275 | 880 MB |
| `Token` | 6,147,240 | 443 MB |
| `AnalyzerNodeInfo` | 2,928,556 | 351 MB |
| `FlowNode` | 1,341,942 | 107 MB |
| `Declaration` | 439,083 | 81 MB |

Estimate, not measured: if even 10% of pyright's 194 GB of allocations became `P::new` in the port, a never-freeing arena would hold about 19 GB.

## recommendations, ranked

1. **Collect the evaluator heap between files.** At the start of each file in `Program._checkTypes` (`analyzer/program.ts`) no evaluator frame is live. A non-moving mark-sweep at that point needs no stack scanning, write barrier or moving, so `P` stays Copy and identity holds. Roots: the program's source files, the evaluator's caches (`typeCache`, `speculativeTypeTracker`, `codeFlowAnalyzerCache` in `createTypeEvaluator`) and thread-locals. Trigger it on arena growth. Switch: collection off. Check mode: poison swept objects without reusing them, and panic on dereference. Sources: Immix (https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf); Bacon et al., "A Unified Theory of Garbage Collection" (https://dl.acm.org/doi/10.1145/1028976.1028982). The collector can come later; its preconditions are the `docs/porting.md` memory rules.
2. **Fewer handles.** The stub generator makes every class a handle by default. Transient objects such as `ConstraintTracker` clones, `DiagnosticAddendum` trees and the speculative stack records in `typeCacheUtils.ts` then leak with their buffers. Make them owned values or `Rc` where pyright never compares them by identity. `TypeResult` stays a handle: `getTypeOfExpression` writes `typeResult.type` after `writeTypeCache` stored the object.
3. **Free tokens after parse.** `SourceFile.parse` keeps `tokenizerOutput` only for client-open files, so the CLI drops the token arrays. Put tokens in a per-file region and keep only the tokens nodes reference. Estimated gain about 1 GB on pydantic-ai's 6,696 files.
4. **Exact-size variants.** A small header plus a details allocation sized to the variant, for `ParseNode`, `Declaration` and `Type`. `TypeBase.cloneType` copies `props` and `priv` and shares `shared`, so a fat type enum would be copied on every clone. Possible later if code reaches details only through typed handles and `d()`. Sources: https://nnethercote.github.io/perf-book/type-sizes.html; https://github.com/ziglang/zig/pull/7920.
5. **Dense type cache.** `typeCache` is a `Map<number, TypeCacheEntry>` keyed by node id that is never iterated, so a per-file slab indexed by node id gives the same results. Check mode: keep the map alongside and panic on a difference.
6. **32-bit scaled handles.** The arena already exceeds 4 GB, so offsets need a 3-bit shift. Source: https://v8.dev/blog/pointer-compression. tsrs measured 14 to 15% less memory for 6.5% more instructions.
7. **Parallel read, tokenize and parse,** with node ids as a per-file base plus a local index, bases assigned in pyright's order. Estimated gain about 3 s of the 5.76 s front end.

## rejected

- **Dropping and re-parsing trees mid-run.** pyright drops trees only together with the evaluator (`Program.emptyCache`). New node ids miss the id-keyed type cache and change evaluation order.
- **Hash-consing types.** `TypeBase.cloneType` returns fresh objects that callers mutate, and results are cached on the source object (`cached.typeBaseInstanceType`). Merging identities changes later writes.
- **Disk offload, mmap, compression.** The live set is touched continuously; the rest is garbage, and freeing beats compressing.
- **Parallel checking.** pyright's `--threads` gives each worker its own evaluator, so per-file cache state differs from a serial run. Optimistic schemes that reproduce a serial order (Block-STM, https://arxiv.org/abs/2203.06871) would conflict on every shared cache miss. Untested: diffing `pyright --threads --outputjson` against the reference.
- **Incremental frameworks** (Salsa, Adapton) need pure queries; speculative rollback and incomplete types are not pure.

Speed beyond 3x comes from single-thread work: no GC or allocation overhead (1, 2), dense tables (5), compact layouts (4).
