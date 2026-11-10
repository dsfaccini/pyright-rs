# shrink notes

PLAN.md step 6, "exact-size variants": cut the `size_of` of the evaluator heap's most-allocated types without changing output.

## Signature changes

- `Type.props` and `Type.cached` are `RefCell<Option<Box<..>>>`. Insert with `get_or_insert_with(Default::default)`.
- `LiteralTypes` maps, `UnionDetailsPriv.type_alias_sources` and `FunctionDetailsPriv.call_site_return_type_cache` are `Option<Box<..>>`.
- `ClassDetailsPriv` keeps its hot fields; `alias_name`, `typed_dict_narrowed_entries`, `is_typed_dict_partial`, `is_asymmetric_descriptor`, `is_asymmetric_attribute_accessor`, `fget_info`, `fset_info`, `fdel_info`, `deprecated_instance_message` and `partial_call_type` live in `ClassDetailsPrivCold`: read `priv_.cold().alias_name`, write `priv_.cold_mut().alias_name = ..`.
- `TypeVarDetailsPriv.scope_id`, `scope_name` and `name_with_scope` are `Option<Rc<str>>`. Compare with `.as_deref()`; set with `.into()`.
- `TypeResult.type_list`, `overloads_used_for_call`, `member_access_deprecation_info` and `magic_method_deprecation_info` are boxed: `Option<Box<..>>`. Convert with `.map(Box::new)` and `.as_deref().cloned()`.

## Shared-file edits

- `tools/stubgen`: `[cold]` in `table.ts` and `emit.ts`; `[sites]` and `[cold]` entries in `typemap.toml`; `docs/evaluator-design.md` "cold fields".
- `docs/porting.md`: "memory" and "strings" rules for small evaluator objects.
- `docs/collector-design.md`: the "no per-object header" line no longer names `Type`'s old size.
- Call sites of the fields above in `checker.rs`, `operations.rs`, `constraint_solver.rs`, `constructor_transform.rs`, `constructors.rs`, `data_classes.rs`, `decorators.rs`, `parameter_utils.rs`, `pattern_matching.rs`, `properties.rs`, `type_evaluator.rs`, `type_guards.rs`, `type_printer.rs`, `type_utils.rs`, `typed_dicts.rs`.

## Needs from others

## Doubts
