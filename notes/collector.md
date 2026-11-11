# collector notes

Collector for the evaluator heap (`PLAN.md` step 6, `docs/collector-design.md`).

## Signature changes

- `P::new` and `P::new_eval` require `T: jsrt::Trace`. `P::new_eval` also requires `T::EVAL` (`#[trace(eval)]`), checked at compile time.
- New in `jsrt`: `Trace` (trait and derive), `Tracer`, `ClassSlot`, `GcMode`, module `heap` (`collect_if_due`, `collect`, `add_root`, `gc_mode`, `eval_heap_live_bytes`). Cargo feature `jsrt/gc-check` compiles check mode's hooks.
- `TypeAliasSharedInfo.computed_variance` lists are allocated with `P::new`: a type that holds no handle cannot be `EVAL`.
- Step 3: `P` is neither `Send` nor `Sync`. `P::to_dyn::<U>` requires `T: Trace`, `U: Trace` and `U::EVAL` when `T::EVAL`. Collection is on by default; `PYRIGHT_RS_GC_RATIO` joins `PYRIGHT_RS_GC_MIN_MIB`.

## Shared-file edits

- `#[derive(jsrt::Trace)]`, and `#[trace(eval)]` where the type is allocated with `P::new_eval`, on 341 types across `crates/pyright/src`, added where the compiler asked for them. The typed-handle macros in `parse_nodes.rs`, `tokenizer_types.rs` and `code_flow_types.rs` derive it too.
- `#[trace(skip)]` on stored closures: `AnalyzerFileInfo.import_lookup`, `ImportLookupResult.doc_string`, `Program.look_up_import`, `TypeEvaluator.import_lookup`, `SourceFile.module_name_getter`, `Scope.chained_module_level_scope_lookup`, `Disposable.dispose`, `MappedEntry.filter`.
- `#[trace(program)]` on `AnalyzerNodeInfo.declaration` and `Declaration.submodule_fallback`.
- `#[trace(holds_eval)]` on `TypeResultWithNode` (it holds `Vec<Self>`).
- `unsafe impl Trace` for `dyn AnalyzerNodeInfoReader` (`analyzer_node_info.rs`) and `dyn FunctionReturnTypeCallback` (`type_printer.rs`).
- `types.rs`: the eight singleton types move from `P::new_eval` to `P::new`.
- `enums.rs`, `protocols.rs`: `ENUM_EVAL_STACK` and `PROTOCOL_ASSIGNMENT_STACK` register collector roots.
- `program.rs`: `Program::analyze` calls `jsrt::heap::collect_if_due()` before each file.
- `oracle/corpus/check.sh`: `CHECK_FEATURES` passes cargo features to the build.
- `docs/porting.md` "memory" and "object references", `docs/evaluator-design.md` section 1: collector rules and wording.

## dependencies

- `libc` (jsrt): `mmap` with `MAP_NORESERVE` and `madvise`, for the heap ranges. V8's heap reserves and releases pages the same way.
- `syn`, `quote`, `proc-macro2` (new crate `jsrt_derive`): the `Trace` derive. All three were already in `Cargo.lock` through `serde_derive`.

## Needs from others

- `tools/stubgen` emits structs without `#[derive(jsrt::Trace)]`. A new struct that a heap object holds fails to compile until it gets one, so the upstream sync (step 7) must add it.

## Doubts

- Sweeping drops about 110M dead objects on pydantic-ai, and most of the 8 to 9 s it takes is glibc `free` of the buffers they own. With mimalloc as the global allocator the sweep took 3.0 s instead of 8.4 s, and the whole check 0:50.7 instead of 1:05.9 (GC off: 0:41.5 instead of 0:51.3). Not adopted: a project-wide dependency, decision raised with the main session.
