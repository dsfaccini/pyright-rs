# flow notes

Share F of step 5: `codeFlowEngine.ts`, `typeGuards.ts` 1-2067, `patternMatching.ts`, `typeCacheUtils.ts`, `typeEvaluatorTypes.ts` (`docs/step5-shares.md`). `typeGuards.ts` 2068-3024 moved to share J.

## Signature changes

All through `tools/stubgen` (`typemap.toml`, generator), documented in `docs/evaluator-design.md`.

- `TypeEvaluator.speculative_type_tracker` and `get_code_flow_engine`'s tracker parameter are `P<RefCell<SpeculativeTypeTracker>>` (`[sites]`). pyright shares one tracker between the evaluator and its code flow engine (typeEvaluator.ts:30591). The evaluator field was `RefCell<..>`, the engine's a by-value copy. Approved by the main session.
- `ContextualTypeCacheEntry` is a trait with accessor `expected_type()` (`[traits]`). `[items]` implements it for `SpeculativeTypeEntry`, `P<T>` and `TypeFormTypeCacheEntry`. `contextual_type_cache_entry_matches` takes `&dyn ContextualTypeCacheEntry`; `add_contextual_type_cache_entry<T: Clone + ContextualTypeCacheEntry>`.
- `typeCacheUtils.ts`'s own `TypeResult` interface merges into the evaluator's `TypeResult` (`[merge]`): `SpeculativeTypeEntry.type_result` and `add_speculative_type`'s parameter are `P<TypeResult>`. The evaluator writes `typeResult.type` after it hands the object to the tracker (typeEvaluator.ts:1349), and `getSpeculativeType` returns that same object.
- `ensure_expected_type_candidates<T: Clone>`.
- `ClassDetailsPriv.tuple_type_args` is `Option<P<RefCell<Vec<TupleTypeArg>>>>` (`[sites]`, share A's field, decided by the main session). Every `cloneType` copy shares the array, and `getSequencePatternInfo` splices it in place (patternMatching.ts:1558-1684); the splice now reaches every clone, as in pyright. `ClassType::specialize` and the pair transform at typeUtils.ts:3675 allocate a new handle.
- Generator:
  - `[bounds]`: a function type parameter with bounds stays a Rust generic.
  - `[mutable]` also applies to factory locals, keyed like `[sites]`.
  - A `[merge]` into a same-named type of another file emits a note, not a self-referential alias.

## Shared-file edits

- `tuple_type_args` readers and writers, for the handle above: `checker.rs`, `constraint_solver.rs`, `constructors.rs`, `data_classes.rs`, `named_tuples.rs`, `operations.rs`, `parameter_utils.rs`, `tuples.rs`, `type_evaluator.rs`, `type_guards.rs` (share J's part too), `type_printer.rs`, `type_utils.rs`, `type_utils_tests.rs`, `type_walker.rs`, `types.rs`.
- `crates/pyright/src/analyzer/mod.rs`: `#[cfg(test)] mod type_cache_utils_tests;`. A test module inside a generated file makes `--install` refuse, so ported tests live in sibling files.

## Needs from others

- Share J: `is_name_same_scope` and `is_matching_expression_or_walrus_rhs` (typeGuards.ts:2986, 3008), which most of `getTypeNarrowingCallback` calls.

## Doubts

- `getTypeFromCodeFlow` returns `typeAtStart` when `flowNode` is undefined. The Rust signature takes a `P<FlowNode>`, and both callers (typeEvaluator.ts:3788, 22506) pass a defined node, so that branch is not ported.
- `narrowTypeForIsEllipsis`: `getBuiltInObject` always returns a type, so the `?? getBuiltInObject(node, 'ellipsis') ?? AnyType.create()` fallbacks never run and are not ported.
- Narrowing callbacks (`TypeNarrowingCallback`) read `isIncomplete` from the captured `P<TypeResult>` when they run, as the TypeScript closures do, not when they are built.
- `getFlowNodeReachability`'s nested `cacheReachabilityResult` keys the cache by the outer call's flow node, and the recursive lookup by the recursive call's: both as in the TypeScript.
- `stubgen --install` rewrites every other share's finished file into stub form (split imports, stub header). After an install, restore the files you do not own before committing.
- `pattern_matching.rs` indexes arrays where JavaScript would read `undefined` out of range (`getTypeOfPatternSequenceEntry`, `getSequencePatternInfo`), so such a read panics in Rust.
- JavaScript `===` on literal values is a nested `literal_values_strictly_equal` in `pattern_matching.rs`: values by value, enum and sentinel literals by identity.
- `preventRecursion` and the loop-label antecedent evaluation in `codeFlowEngine.ts` catch, clean up and rethrow. Only cancellation and JS stack overflow throw there; the port has neither, so the catches are not ported (porting.md "exceptions").
- `SpeculativeTypeTracker`'s methods take `&mut self` (generator rule for owned classes), even the read-only ones. `get_speculative_type` copies the entry handles before calling `_dependent_types_match(&mut self)`.
