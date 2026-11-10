# eval-1: share B, typeEvaluator.ts:1-7677

## Signature changes

- `TypeResult::new(type_)` and `impl Clone for TypeResult`, added through `tools/stubgen/typemap.toml` `[items]` (approved by the main session). TypeScript builds a `TypeResult` as an object literal or a spread; Rust writes `P::new_eval(TypeResult { field: .., ..TypeResult::new(t) })` and `P::new_eval((*r).clone())`.
- `TypeResultWithNode::new(type_, node)` and `TypeResultWithNode::from_type_result(&TypeResult, node)`, the same way (requested by the main session for eval-2): `{ type, node }` and `{ ...typeResult, node }`.
- Generator rule (`tools/stubgen/src/emit.ts` `valueDerive`): a value or inline-object struct whose fields are all optional derives `Default` as well as `Clone` (32 structs: `TypeSameOptions`, `ApplyTypeVarOptions`, `PartialPrefetchedTypes`, the `*Priv` records, ...).

## Shared-file edits

- `crates/pyright/src/analyzer/type_evaluator_types.rs` (share F): the `[items]` impls above, installed by stubgen.
- `docs/evaluator-design.md`: `[items]` may also hold an `impl` that stands in for an object literal; all-optional value structs derive `Default`.
- Derive lines of the 32 all-optional value structs in `types.rs`, `type_utils.rs`, `type_evaluator_types.rs`, `type_cache_utils.rs`, `code_flow_engine.rs`, `operations.rs`, `parameter_utils.rs`, `type_printer.rs`, `type_evaluator.rs`, installed by stubgen.
- `docs/step5-shares.md`: `--install` alone does not regenerate; regenerate first. How to build a `TypeResult`.

## Needs from others

## Doubts

- `read_contextual_type_cache_entry_for_node` and `is_type_form_type_cached` return or check a `TypeFormTypeCacheEntry` as the `TypeCacheEntry` it extends (`docs/evaluator-design.md` section 6, flattened interface inheritance): a conversion that drops `expected_type`. Their callers read only `type_result`, `incomplete_gen_count` and `flags`.
- `read_type_cache`'s flag-mismatch message (debug only, `verifyTypeCacheEvaluatorFlags`) prints node types as enum numbers, like TypeScript, and `console.log` goes to stdout.
- `add_diagnostic` and `add_diagnostic_for_text_range` read the rule level with `get_level(rule).unwrap()`. TypeScript casts a boolean rule to a level and carries on; Rust panics. No caller passes a boolean rule.
- `expand_typed_kwargs` truncates and appends the parameter array that `FunctionType.clone` shares with the original, as TypeScript does (share A's shared parameter arrays).
- `get_declared_type_for_expression`, Index case: the TypeScript `break` out of the switch is a labeled block, so control reaches the final `if (symbol)`.
- `assign_type_to_member_variable` returns early where TypeScript nests the rest under one `if`; nothing follows the block.
- `promotionTypeName.split('.')` uses `str::split`: same result as JavaScript on these ASCII names.
- `get_type_of_expression` writes `inference_context.expected_type` through `Option<&mut InferenceContext>` (typeEvaluator.ts:1251). eval-2's change makes `InferenceContext` a handle; the write and its pass-throughs convert then.
- `get_type_of_ellipsis` drops `?? getBuiltInObject(node, 'ellipsis') ?? AnyType.create()` fallbacks: `getBuiltInObject` always returns a type, so they never run in pyright either.
- `getTypeOfStringList` joins string parts as UTF-16 units before converting, so a surrogate pair split across two parts survives.
