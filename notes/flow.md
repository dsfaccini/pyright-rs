# flow notes

Share F of step 5: `codeFlowEngine.ts`, `typeGuards.ts`, `patternMatching.ts`, `typeCacheUtils.ts`, `typeEvaluatorTypes.ts` (`docs/step5-shares.md`).

## Signature changes

All through `tools/stubgen` (`typemap.toml`, generator), documented in `docs/evaluator-design.md`.

- `TypeEvaluator.speculative_type_tracker` and `get_code_flow_engine`'s tracker parameter are `P<RefCell<SpeculativeTypeTracker>>` (`[sites]`). pyright shares one tracker between the evaluator and its code flow engine (typeEvaluator.ts:30591). The evaluator field was `RefCell<..>`, the engine's a by-value copy. Approved by the main session.
- `ContextualTypeCacheEntry` is a trait with accessor `expected_type()` (`[traits]`). `[items]` implements it for `SpeculativeTypeEntry`, `P<T>` and `TypeFormTypeCacheEntry`. `contextual_type_cache_entry_matches` takes `&dyn ContextualTypeCacheEntry`; `add_contextual_type_cache_entry<T: Clone + ContextualTypeCacheEntry>`.
- `typeCacheUtils.ts`'s own `TypeResult` interface merges into the evaluator's `TypeResult` (`[merge]`): `SpeculativeTypeEntry.type_result` and `add_speculative_type`'s parameter are `P<TypeResult>`. The evaluator writes `typeResult.type` after it hands the object to the tracker (typeEvaluator.ts:1349), and `getSpeculativeType` returns that same object.
- `ensure_expected_type_candidates<T: Clone>`.
- Generator:
  - `[bounds]`: a function type parameter with bounds stays a Rust generic.
  - `[mutable]` also applies to factory locals, keyed like `[sites]`.
  - A `[merge]` into a same-named type of another file emits a note, not a self-referential alias.

## Shared-file edits

- `crates/pyright/src/analyzer/mod.rs`: `#[cfg(test)] mod type_cache_utils_tests;`. A test module inside a generated file makes `--install` refuse, so ported tests live in sibling files.

## Needs from others

- Share A: `TypeVarType::create_instance` and `is_type_same`. The two tests in `type_cache_utils_tests.rs` are `#[ignore]`d until they land.

## Doubts

- `preventRecursion` and the loop-label antecedent evaluation in `codeFlowEngine.ts` catch, clean up and rethrow. Only cancellation and JS stack overflow throw there; the port has neither, so the catches are not ported (porting.md "exceptions").
- `SpeculativeTypeTracker`'s methods take `&mut self` (generator rule for owned classes), even the read-only ones. `get_speculative_type` copies the entry handles before calling `_dependent_types_match(&mut self)`.
