# eval-2 notes

Share C of step 5: the bodies under `typeEvaluator.ts:7678-15241` in `analyzer/type_evaluator.rs`.

## Signature changes

- `Arg` accessors through `typemap.toml` `[items]`: `arg_category()`, `node()`, `name()`, `type_result()`, `value_expression()`, `active()`, `enforce_iterable()`. TypeScript reads these `ArgBase` members on the `Arg` union; each accessor reads the member from whichever variant holds the argument.

- `InferenceContext` is a handle (`[modes]`, main session's decision): every `inference_context` parameter is `Option<P<InferenceContext>>`, `make_inference_context` returns `Option<P<InferenceContext>>`, `expected_type` is a `Cell`. Record in `docs/evaluator-design.md`, section 3, objects.
- `TypeAliasSharedInfo.computed_variance` (share A's field, approved by the main session) is `Cell<Option<P<RefCell<Vec<Variance>>>>>`; `infer_variance_for_type_alias` returns `Option<P<RefCell<Vec<Variance>>>>` and `update_usage_variances_recursive` takes `usage_variances: P<RefCell<Vec<Variance>>>` (`[sites]`).
- stubgen: the run report lists value-mode types written through a parameter (`Gen.writtenThroughParamSites`).

## Shared-file edits

- `tools/stubgen/typemap.toml`: the `Arg` item above.
- `crates/pyright/src/analyzer/type_evaluator_types.rs`: the installed `Arg` item.
- `tools/stubgen/src/gen.ts`, `main.ts`: the report above.
- `crates/pyright/src/analyzer/type_evaluator.rs`, `tuples.rs`, `constructor_transform.rs`, `enums.rs`: ported functions of shares B, D, E and H on main (`get_type_of_expression` and its callers, the dictionary and list-or-set inference paths, `get_type_of_arg`, the type alias placeholder, the variance reads at typeEvaluator.ts:1062 and 27067): mechanical updates to `Option<P<InferenceContext>>`, `expected_type.get()`/`.set()`, and the shared `computed_variance`. Only the `todo!()` stubs whose signature changed were regenerated; the install's rustfmt pass over other shares' code was left out.
- `crates/pyright/src/analyzer/{type_utils,constructors,operations,tuples,type_evaluator}.rs`: the installed `InferenceContext` signatures. Other files the install rewrote (share H's ported files reformatted) were left as on main.
- `stubgen --install` drops the hand-added `use crate::{analyzer, common, parser};` line of `type_evaluator.rs`, `types.rs` and `type_printer.rs`; this branch restores it by hand after the install.

## Needs from others

- Every share C body calls functions of other shares that may still be `todo!()`; nothing ran yet with `PYRIGHT_RS_CHECKER=1`.

## Doubts

Judgment calls made while porting, by TypeScript line. Each keeps pyright's observable behavior as far as the port can tell.

- 8056 `getTypeOfIndexWithBaseType`: pyright returns the `{ type, node }` object from `createSpecializedTypeAlias` as the `TypeResult`; the port returns a fresh `TypeResult` with only `type` set.
- 8719 `getTypeArgs`: the nested `getTypeArgTypeResult` writes the outer `adjFlags`; the port captures it in an `FnMut` closure, so later arguments see the write.
- 8540 `getTypeOfIndexedObjectOrClass`: `typeof literalValue === 'number'` matches `LiteralValue::Number` only, so a bigint literal takes the slice branch, as in pyright.
- 9273, 9290, 9550: `priv.literalValue` is read before the `ClassType.isBuiltIn` / `isSameGenericClass` checks, so no `RefCell` borrow spans those calls. Both are pure reads.
- 11030, 11063 `expandArgTypes`: a read past the array end is `undefined` and a write past it grows the array, as in JavaScript. Callers pass arrays of one length.
- 11350, 11373 `validateCallForFunction`: an undefined `skipUnknownArgCheck` passed to `validateArgs` takes the default `false`.
- 10019, 10817: `MatchArgsToParamsResult` handles are shared as pyright shares the objects; an argument-type override builds a new handle with the fields copied, as the TypeScript spread does.
- 12085 `matchArgsToParams`: index locals are `i32` so `findIndex`'s -1 compares as in TypeScript; the initial `paramCategory` value, never read, is left out; TypeScript `assert` is `assert!`.
- 13878 `validateArgType`: `message + diag?.getString()` appends `"undefined"` without a `diag`, as JavaScript does; pyright never reaches that branch.
- 14179, 14420, 14512, 14656, 14821: a name joined from string-list parts decodes the UTF-16 units once, so a lone surrogate becomes U+FFFD (porting.md, strings, known gap). `createTypeAliasType` compares names as UTF-16.
- JavaScript truthiness of an optional string (`if (paramName)`, scope ids) treats `""` as absent throughout.
- TypeScript non-null assertions (`x!`) are `unwrap()` or `expect(..)`; they panic where JavaScript would throw later.
