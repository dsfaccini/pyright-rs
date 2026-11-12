# temps notes

Fewer dead evaluator temporaries that cost the sweep, and a cheaper mutator side for the ones that remain, with pyright 1.1.414's output unchanged.

## Signature changes

- `ConstraintSolutionSet._type_var_map` is a `OnceCell<P<TypeVarSolutions>>`; `ConstraintSet._type_var_map` and `_scope_ids` are `OnceCell<P<TypeVarConstraintMap>>` and `OnceCell<P<ScopeIdSet>>`. All private to their files.
- `TypeResult.cold` and `FunctionDetailsShared.cold` are `jsrt::EvalOptionRefCell<..Cold>`; `into_cold` returns one. `cold()`, `cold_mut()` and `set_cold` keep their signatures.
- `DiagnosticAddendum.content` is a `OnceCell<P<AddendumContentPart>>` (private).
- `jsrt::VecMap<K, V>`: the linear map `LazyJsMap` boxes, without the box. `jsrt::EvalOptionRefCell<T>`: `OptionRefCell`'s API with its storage on the evaluator heap. `jsrt::cell::OnceCell<T>: Default` for every `T`.

## Shared-file edits

- `crates/jsrt/src/collections.rs`, `cell.rs`, `lib.rs`: `VecMap`, `EvalOptionRefCell`, `OnceCell: Default`.
- `crates/pyright/src/analyzer/constraint_solution.rs`, `constraint_tracker.rs`, `type_evaluator_types.rs`, `types.rs` (`FunctionDetailsShared` only), `common/diagnostic.rs`.
- `tools/stubgen/src/emit.ts`, `tools/stubgen/typemap.toml`: a handle struct's cold half is emitted as `jsrt::EvalOptionRefCell`. `tools/stubgen/out/` is not regenerated.
- `docs/porting.md` ("memory"), `docs/evaluator-design.md` ("cold fields"), `docs/collector-design.md` ("sweep cost").

## Needs from others

## Doubts

- No switch restores the boxed parts. The change is to how a value is held, not to what any operation returns, like `shrink`'s cold halves and the interned atoms, which have none; a runtime switch would have to give the types back the drop glue the change removes. The gates compare the output and every dump stage, and check mode verifies that every part stays reachable to marking and that the write barrier sees writes into parts.

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
