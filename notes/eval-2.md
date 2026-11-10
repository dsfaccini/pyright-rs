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

## Doubts
