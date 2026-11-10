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
