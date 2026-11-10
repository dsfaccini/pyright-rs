# shrink notes

PLAN.md step 6, "exact-size variants": cut the `size_of` of the evaluator heap's most-allocated types without changing output.

## Signature changes

- `Type.props` and `Type.cached` are `RefCell<Option<Box<..>>>`. Insert with `get_or_insert_with(Default::default)`.
- `LiteralTypes` maps, `UnionDetailsPriv.type_alias_sources` and `FunctionDetailsPriv.call_site_return_type_cache` are `Option<Box<..>>`.
- `ClassDetailsPriv` keeps its hot fields; `alias_name`, `typed_dict_narrowed_entries`, `is_typed_dict_partial`, `is_asymmetric_descriptor`, `is_asymmetric_attribute_accessor`, `fget_info`, `fset_info`, `fdel_info`, `deprecated_instance_message` and `partial_call_type` live in `ClassDetailsPrivCold`: read `priv_.cold().alias_name`, write `priv_.cold_mut().alias_name = ..`.
- `TypeVarDetailsPriv.scope_id`, `scope_name` and `name_with_scope` are `Option<Rc<str>>`. Compare with `.as_deref()`; set with `.into()`.
- `TypeResult` keeps `type_`, `is_incomplete`, `type_errors`, `class_type` and the flags; `bind_to_self_type`, `unpacked_type`, `type_list`, `inlined_type_dict`, `is_empty_tuple_shorthand`, `expected_type_diag_addendum`, `narrowed_type_for_set`, `overloads_used_for_call` and the two deprecation infos live in `TypeResultCold`, behind `RefCell<Option<Box<..>>>`. Read `tr.cold().unpacked_type`, write `tr.set_cold(|cold| &mut cold.f, v)` or `tr.cold_mut().f = v`, build with `cold: TypeResultCold { .. }.into_cold()`.
- `FunctionDetailsShared.doc_string`, `deprecated_message`, `decorator_data_class_behaviors` and `inferred_return_type` live in `FunctionDetailsSharedCold` the same way.
- `ConstraintSet._type_var_map` and `ConstraintSolutionSet._type_var_map` are `jsrt::LazyJsMap`, which allocates its table on the first insert and offers the `JsMap` methods these files use. `ConstraintSet._scope_ids` is `Option<Box<JsSet>>`. `ConstraintTracker._constraint_sets` is `Box<[P<ConstraintSet>]>`: it is only replaced whole, cloned and indexed.
- `DiagnosticAddendum` keeps its messages, children and range in one `AddendumContent`, allocated on the first write. Its fields were private, so no caller changed.

## Shared-file edits

- `crates/jsrt/src/collections.rs`: `LazyJsMap`.
- `crates/pyright/src/common/diagnostic.rs`: `DiagnosticAddendum` layout.
- `tools/stubgen`: `[cold]` for value and handle structs in `table.ts` and `emit.ts`; `[sites]` and `[cold]` entries in `typemap.toml`; `docs/evaluator-design.md` "cold fields".
- `docs/porting.md`: "memory" and "strings" rules for small evaluator objects.
- `docs/collector-design.md`: the "no per-object header" line no longer names `Type`'s old size.
- Call sites of the fields above in `checker.rs`, `operations.rs`, `tuples.rs`, `constraint_solver.rs`, `constructor_transform.rs`, `constructors.rs`, `data_classes.rs`, `decorators.rs`, `parameter_utils.rs`, `pattern_matching.rs`, `properties.rs`, `type_evaluator.rs`, `type_guards.rs`, `type_printer.rs`, `type_utils.rs`, `typed_dicts.rs`.

## Needs from others

- collector: `jsrt::LazyJsMap` needs a `Trace` impl, like `JsMap`. `DiagnosticAddendum`'s children sit behind `RefCell<Option<Box<AddendumContent>>>`.

## Doubts

- A handle's `cold()` borrow lives to the end of its statement, or through the body of an `if let`. Every `if let` on a cold field now copies the value out first. A call argument `f(r.cold().x)` where `f` writes `r`'s cold half would still panic; the seven corpora hit none.
