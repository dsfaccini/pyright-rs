# temps notes

Fewer dead evaluator temporaries that cost the sweep, and a cheaper mutator side for the ones that remain, with pyright 1.1.414's output unchanged.

## Signature changes

- `ConstraintSolutionSet._type_var_map` is a `OnceCell<P<TypeVarSolutions>>`; `ConstraintSet._type_var_map` and `_scope_ids` are `OnceCell<P<TypeVarConstraintMap>>` and `OnceCell<P<ScopeIdSet>>`. All private to their files.
- `TypeResult.cold` and `FunctionDetailsShared.cold` are `jsrt::EvalOptionRefCell<..Cold>`; `into_cold` returns one. `cold()`, `cold_mut()` and `set_cold` keep their signatures.
- `DiagnosticAddendum.content` is a `OnceCell<P<AddendumContentPart>>` (private).
- `ClassDetailsPriv.type_args` is an `Option<TypeArgs>` (`jsrt::EvalShared<TypeArgList>`, a Copy handle that derefs to `[P<Type>]`); `LiteralValue::String` and `Bigint` hold `jsrt::EvalShared<LiteralString>` and `<LiteralBigInt>`, which deref to `JsString` and `JsBigInt`. `alias_name` and `is_typed_dict_partial` are fields of `ClassDetailsPriv` itself. Its `cold()` returns a borrow (`OptionRefOr`) and `cold_mut()` a `RefMut`. `Type.props` and `Type.cached` are `jsrt::EvalOptionRefCell`s.
- `jsrt::EvalShared<S>`: a Copy handle to an immutable evaluator object `S` that derefs to `S::Target`; compares, hashes and prints by content.
- `jsrt::VecMap<K, V>`: the linear map `LazyJsMap` boxes, without the box. `jsrt::EvalOptionRefCell<T>`: `OptionRefCell`'s API with its storage on the evaluator heap. `jsrt::cell::OnceCell<T>: Default` for every `T`.

## Shared-file edits

- `crates/jsrt/src/collections.rs`, `cell.rs`, `lib.rs`: `VecMap`, `EvalOptionRefCell`, `OnceCell: Default`.
- `crates/jsrt/src/heap.rs`: the allocation prefetch. `crates/jsrt/src/arena.rs`: `EvalShared`.
- `crates/pyright/src/analyzer/constraint_solution.rs`, `constraint_tracker.rs`, `type_evaluator_types.rs`, `types.rs`, `common/diagnostic.rs`; for the class-type fields, read sites in `type_evaluator.rs`, `type_utils.rs`, `checker.rs`, `type_printer.rs`, `typed_dicts.rs`, `code_flow_engine.rs`, `constraint_solver.rs` and a few more (redundant `.clone()`s of the now-Copy type arguments, removed by `clippy --fix`).
- `tools/stubgen/src/emit.ts`, `tools/stubgen/typemap.toml`: every cold half is emitted as `jsrt::EvalOptionRefCell`; `Type.props`, `Type.cached` and the literal values map to the new types; `aliasName` and `isTypedDictPartial` leave `ClassDetailsPriv`'s cold list. `tools/stubgen/out/` is not regenerated.
- `docs/porting.md` ("memory"), `docs/evaluator-design.md` ("cold fields"), `docs/collector-design.md` ("sweep cost").

## Needs from others

## Doubts

- No switch restores the boxed parts, the `Rc`s or the allocation prefetch. Each change is to how a value is held or where memory is prefetched, not to what any operation returns, like `shrink`'s cold halves and the interned atoms, which have none; a runtime switch would have to give the types back the drop glue the change removes. The gates compare the output and every dump stage, and check mode verifies that every part stays reachable to marking and that the write barrier sees writes into parts.

## Where the sweep went

`main` 04ac81d, pydantic-ai, release, `PYRIGHT_RS_GC_LOG=2`: 10 collections freed 105.8 M objects. `TypeResult` 18.3 M, `ConstraintSet` 15.2 M, `DiagnosticAddendum` 14.6 M, `ConstraintSolutionSet` 10.0 M, `ConstraintTracker` 7.8 M: 62% of them. Pauses 2.0 s, of which marking 0.65 s and the sweep and the rest 1.36 s.

Cycle samples (`perf record -e cycles:u`, all threads): the per-type sweep functions are 2.79% of all cycles; those of `DiagnosticAddendum` 0.82%, `ConstraintSet` 0.37%, `TypeResult` 0.31%, `ConstraintSolutionSet` 0.27%. Each of the four had drop glue only to free a box most of its objects never allocate: the sweep read the slot of every dead one to find out. `ConstraintTracker`'s sweep is 0.03%.

The mutator pays for the same objects when it allocates them: `P<TypeResult>::new_eval` is 1.76% of cycles, two thirds of it the first store into the reused, cache-cold slot.

## Attempts

pydantic-ai, release; instructions from single runs (they repeat within about 0.1%), pauses from `PYRIGHT_RS_GC_LOG=1`; wall and cycles from runs paired under one hold of the pydantic-ai lock.

| build | instructions | collections | pauses (marking) | notes |
|---|---|---|---|---|
| `main` 04ac81d | 212.99 G | 10 | 2.01 s (0.65 s) | |
| constraint sets' maps and scope ids as evaluator objects (5742916) | 212.89 G | 10 | 1.66 s (0.59 s) | |
| also the cold halves of `TypeResult` and `FunctionDetailsShared` (be99437) | 212.19 G | 10 | 1.50 s (0.58 s) | |
| also `DiagnosticAddendum`'s content (042ddfd) | 212.79 G | 11 | 1.49 s (0.64 s) | the parts add about 0.5 GiB of slots, so one collection more |

- 042ddfd against be99437, three pairs at load 2.8 to 4.4: wall -0.1 to -0.5 s, peak RSS -0.15 GiB. Kept, though it runs 0.6 G more instructions: the extra collection.
- 042ddfd against `main`, three pairs at load 3.4 to 4.4: median wall 29.14 to 28.27 s (-3.0%), cycles 146.95 to 141.34 G (-3.8%), pauses 2.08 to 1.57 s, peak RSS 5.69 to 5.52 GiB.
- Cycle samples at 042ddfd: the sweep functions 1.46% of all cycles (2.79% before); `mi_free` 2.85 to 2.41%, `_mi_page_malloc_zero` 3.56 to 2.80%: the parts no longer come from malloc. IBS samples of ops that miss L3: 5,301 against 6,293.
- samples, five runs each, interleaved: 7.548 G to 7.562 G instructions (+0.20%; samples never collects, so it pays the allocation of the parts and no sweep), peak RSS 0.451 to 0.446 GiB.

## Allocation prefetch

At 042ddfd `P<TypeResult>::new_eval` was still 1.64% of cycles, two thirds of it the first store into the reused slot. The cursor prefetched the first and last lines of the slot two allocations ahead; for a 24-byte slot that is 48 bytes ahead, mostly the line being written. Each variant below prefetches `here + N` on every allocation; pairs at load 4.6 to 6.4 (the Miri suites ran alongside).

| variant | against | instructions | cycles | wall |
|---|---|---|---|---|
| also 256 bytes ahead | 042ddfd | +0.11 G | | -0.48, -0.23 s |
| 512 against 128 bytes ahead | | +0.03 to +0.15 G | -2.5%, -3.4% | -0.82, -0.98 s |
| 1024 against 512 bytes ahead | | +0.01 to +0.09 G | +1.8%, -2.1% | +0.56, -0.67 s: no better |
| 512 bytes ahead | 042ddfd | +0.14 G | -4.4%, -1.0% | -1.24, -0.14 s |
| 512 bytes ahead, without the slot-two-ahead prefetch (kept, 35293de) | 512 with it | -1.15 G (-0.54%) | -0.4%, -1.2% | -0.08, -0.31 s |

- 35293de against 042ddfd: 212.79 G to 211.82 G instructions (-0.47%); six more pairs at load 9 to 14 (Miri runs alongside): wall -3.58, +0.29, -0.73, +0.13, -2.06, +1.66 s (median -0.3 s), cycles median -0.6%, pauses -0.06 to -0.16 s in the last three, peak RSS unchanged (5.52 GiB).
- samples: 7.562 G to 7.537 G instructions (median of 6 interleaved), peak RSS 0.446 GiB on both; it never collects, so its allocations come from fresh pages, where neither prefetch matters much.

## Dead class types

At 35293de, `TypeAlloc<ClassTypeDetails>` is the type the sweep spends most on (0.47% of cycles, 10.2 M dead over a pydantic-ai run). Counted in a diagnostic build over the first 8.0 M dead ones: 69% own something a drop must free. The type arguments' `Rc` 45%, the header's `cached` 29%, a string or big integer literal 8%, the header's `props` 4%, the cold half 1%. The sweep skips a type's slots only when no field has drop glue, so all five had to go.

- Prototype, all five as evaluator objects: +2.4 G instructions on pydantic-ai, +180 M (+2.4%) on samples, which never collects. `ClassType::is_same_generic_class`, 9.4% of samples' instructions, rose to 11.4%: it reads `cold().is_typed_dict_partial` on every call, and `is_built_in` reads `cold().alias_name`, now through a borrowed cell where they were a box's `as_deref`.
- So the two fields move back to the hot half, which has room once the type arguments are a thin 8-byte handle instead of a 16-byte `Rc<[P<Type>]>`.

As landed, each against the commit before; samples median of 2 to 4 interleaved runs, pydantic-ai pairs at the load given:

| commit | samples | pydantic-ai |
|---|---|---|
| 245b9a9: type arguments as `jsrt::EvalShared` | 7.533 to 7.522 G (-0.13%) | not paired alone |
| 63b9ac9: `aliasName`, `isTypedDictPartial` hot | 7.522 to 7.429 G (-1.24%) | both against 35293de, load 9 to 13: 211.82 to 209.13 G (-1.3%); wall -2.31, +1.32, +0.51 s; pauses -0.37, -0.03, -0.03 s; peak RSS +20 to +50 MB |
| 9c60245: no drop glue | 7.430 to 7.431 G | load 7 to 9: 209.10 to 208.97 G (medians); 12 collections instead of 11; pauses -0.19, -0.23, -0.24 s; wall +0.69, -0.69, +0.35 s; peak RSS -80 MB. Three earlier pairs at load 13 to 14: the sweep and the rest of the pauses -0.36, -0.13, -0.26 s |

- 9c60245's pause drop holds in every pair, through the extra collection the new parts' slots bring. Its wall change is within noise.

## Not landed

- **`DiagnosticAddendum` reference counted, off the collected heap.** It holds no evaluator handle, so an `Rc<DiagnosticAddendum>` needs no tracing and no barrier, and an addendum is freed on the mutator when its last handle drops, while still in cache. About 150 call sites change, mostly `Option<P<..>>` parameters that now clone. Against 042ddfd: 212.79 to 213.95 G instructions (+0.55%: malloc, free and counts for 22 M allocations), 10 collections instead of 11, peak RSS +0.1 GiB, wall -1.9, +0.1, -0.4 s in three pairs at load 7.5 to 9.5. Instructions higher with no clear wall gain.

## At the end

The batch tip merged with `main` 00fc773 (the `bind` agent's front end), against 00fc773; pydantic-ai in three pairs at load 12 to 14:

- pydantic-ai: 200.45 G to 197.62 G instructions (-1.4%); pauses -0.30, -0.45, -0.32 s with 12 collections instead of 11; wall -0.28, -1.89, +1.05 s, within noise at this load; peak RSS 5.53 to 5.47 GiB.
- samples, three runs each: 7.208 G to 7.112 G (-1.3%), peak RSS 0.444 to 0.440 GiB.
- Cycle samples at the tip: the sweep functions are 0.99% of all cycles (2.79% at 04ac81d); the largest are `AddendumContentPart` 0.27%, `RefCell<Vec<P<Type>>>` 0.16%, unions 0.09%.

From 04ac81d over the whole lane, pydantic-ai without `bind`'s changes: instructions 212.99 G to 209.02 G (-1.9%), peak RSS 5.69 to 5.48 GiB. Pauses fell by 0.51 s (median, load 3.4 to 4.4), then 0.06 to 0.16 s, then 0.38 to 0.42 s (load 16 to 19), each in its own pairs: an estimate of 2.0 s to about 1.1 s, never measured in one pair.

## Remaining

- Addendum contents are now the sweep's largest drop: messages own `String`s and parameter vectors. The `Rc` attempt above freed them on the mutator but cost more instructions than it saved.
- None of the wall times here is from a quiet devbox: the last pairs ran at load 7 to 19, beside Miri runs and other agents' gates. The pause changes are consistent in sign; the cycle and wall changes are not.
