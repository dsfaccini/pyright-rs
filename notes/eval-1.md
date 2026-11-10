# eval-1: share B, typeEvaluator.ts:1-7677, then typeUtils.ts:1-4746

## Signature changes

- `TypeResult::new(type_)` and `impl Clone for TypeResult`, added through `tools/stubgen/typemap.toml` `[items]` (approved by the main session). TypeScript builds a `TypeResult` as an object literal or a spread; Rust writes `P::new_eval(TypeResult { field: .., ..TypeResult::new(t) })` and `P::new_eval((*r).clone())`.
- `TypeResultWithNode::new(type_, node)` and `TypeResultWithNode::from_type_result(&TypeResult, node)`, the same way (requested by the main session for eval-2): `{ type, node }` and `{ ...typeResult, node }`.
- Generator rule (`tools/stubgen/src/emit.ts` `valueDerive`): a value or inline-object struct whose fields are all optional derives `Default` as well as `Clone` (32 structs: `TypeSameOptions`, `ApplyTypeVarOptions`, `PartialPrefetchedTypes`, the `*Priv` records, ...).
- `MatchArgsToParamsResult` becomes a handle (main session's rule): `P<MatchArgsToParamsResult>`, `argument_match_score: Cell<f64>`, `arg_params: RefCell<Vec<..>>`; `MatchedOverloadInfo.match_results` and the parameters of `validate_overloads_with_expanded_types`, `match_args_to_params`, `validate_arg_types_with_context`, `validate_arg_types_with_expected_type`, `validate_arg_types` follow (share C's functions). Reason in `docs/evaluator-design.md` section 3.
- `tools/stubgen` install (`merge.ts`, `check.ts`): an unported `todo!()` item or a generated struct or enum at a marker the new stub has is regenerated, so a signature or field change no longer reads as dropped code.
- `TypeVarTransformer` (typeUtils.ts:3687) is a trait class (`[traits]`, receiver `&mut self`): `apply` and the transform methods call the six subclasses' overrides through `this`. Generator rule: a `&mut self` trait-class method's callback receives `&mut Self` (`do_for_each_constraint_set`), in the trait and in each implementation. Six override `recursionCount` number sites reviewed as `u32`.
- `validate_type_var_default(.., invalid_type_vars: &mut JsSet<String>)` (typemap `[sites]`, was `&JsSet`): `TypeVarDefaultValidator` adds to the caller's set (typeUtils.ts:4271) and the caller reads it back (typeEvaluator.ts:19681); the set moves into the validator for `apply` and back.
- `tools/stubgen` install (`merge.ts`, `check.ts`): a ported crate file is the base of the merge; only unported items change, nothing else is rewritten or reformatted, and a marked local function inside a ported body is part of that body. On main (3d0b74d) `--install` changes no file.

## Shared-file edits

- `crates/pyright/src/analyzer/type_evaluator_types.rs` (share F): the `[items]` impls above, installed by stubgen.
- `docs/evaluator-design.md`: `[items]` may also hold an `impl` that stands in for an object literal; all-optional value structs derive `Default`.
- Derive lines of the 32 all-optional value structs in `types.rs`, `type_utils.rs`, `type_evaluator_types.rs`, `type_cache_utils.rs`, `code_flow_engine.rs`, `operations.rs`, `parameter_utils.rs`, `type_printer.rs`, `type_evaluator.rs`, installed by stubgen.
- `docs/step5-shares.md`: `--install` alone does not regenerate; regenerate first. How to build a `TypeResult`.
- `crates/pyright/src/analyzer/type_evaluator.rs`, share D's `validate_type_param_default` (typeEvaluator.ts:19681): `let mut invalid_type_vars` and `&mut invalid_type_vars` for the signature change above.
- `crates/pyright/src/analyzer/mod.rs`: `#[cfg(test)] mod type_utils_tests;`.

## Needs from others

- Share H (`type_walker.rs`): the default visit bodies as free functions (`visit_class_base<W: TypeWalker + ?Sized>(w, t)`), so the typeUtils walker overrides can call `super.visitClass` and `super.visitFunction`. Until then `ContainsLiteralTypeWalker`, `AnyWalker` and `AnyOrUnknownWalker` inline copies of those default bodies, with a comment.

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
- `get_type_of_class_member_name`: TypeScript `selfType ?? memberName === '__new__' ? undefined : classType` parses as `(selfType ?? (memberName === '__new__')) ? undefined : classType`; ported as parsed.
- `adjust_type_args_for_type_var_tuple`: the final `typeArgs.push(...)` writes the caller's array while the local array has not been replaced; the port pushes into the caller's `&mut Vec` in that case. Index arithmetic follows JavaScript `slice` rules, negative indexes included.
- A message built only behind `diag?.` is built inside `if let Some(diag)`, so `print_type` (which can infer return types) runs only when TypeScript runs it.
- typeUtils `all_subtypes` on a union: the TypeScript `every` callback returns nothing, so pyright tests only the first subtype and returns false (true for an empty union). Ported as written.
- typeUtils `compare_types` passes recursion count 0 to its recursive calls for functions and overloads, and the real count for class type args, as TypeScript does; strings compare in UTF-16 order (`JsStr::js_cmp`).
- typeUtils `add_signature` repeats `findSignature`'s lookup to get the index it writes, because Rust `find_signature` returns a copy where TypeScript returns the shared entry.
- typeUtils `super.transformTypeVarsInFunctionType` (the only `super` call, `UniqueFunctionSignatureTransformer`, marker 4293): a local `Super` view struct inside the override implements `TypeVarTransformer` by forwarding the accessors and `apply` to the real transformer, so the trait's default body runs and its nested `apply` calls still reach the subclass's overrides.
- typeUtils `TypeVarDefaultValidator` owns `_invalid_type_vars`; TypeScript shares the caller's Set. `validate_type_var_default` copies it in and reads it back after `apply`.
- typeUtils `transformGenericTypeAlias` compares `type !== updatedType`, not the type argument: a pyright quirk, kept.
- typeUtils `apply_solved_type_vars` gets `&ConstraintSolution` but `ApplySolvedTypeVarsTransformer::new` takes `Rc<ConstraintSolution>`: it builds a new `Rc` over the same solution-set handles. The transformer only reads the solution.
- typeUtils `get_class_member_iterator` and `get_class_iterator` are lazy `std::iter::from_fn` state machines: each `next()` runs to the next TypeScript `yield`, and the MRO is read by index like a live array.
- typeUtils `transform_type_pair_recursive`: TypeScript's `try`/`finally` runs the body in a closure, then removes the pending entry.
- typeUtils TypeVar shared spread (`{ ...sourceType.shared, constraints, boundType, defaultType }`) is a full `TypeVarDetailsShared` literal: a new field in share A's struct breaks compilation there, by design.
- typeUtils strings keep JavaScript truthiness: an empty alias name, class name, parameter name or scope id counts as absent.

