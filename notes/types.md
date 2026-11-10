# types notes

Share A of step 5: `analyzer/types.ts` (`docs/step5-shares.md`). `typeUtils.ts` moved to eval-1 before any of its bodies were ported here.

## Signature changes

- `analyzer/symbol.ts:SymbolTable` maps to the `SymbolTable` handle of `analyzer/symbol.rs` (`[types]` in `typemap.toml`). A module type holds its scope's table (typeEvaluator.ts:24280), `isTypeSame` compares module tables with `===` (types.ts:3766), and spread copies of a type share the table. Changed: `ModuleDetailsPriv.fields` and `loader_fields`, `ModuleType::create`, `ClassDetailsShared.fields` (`Cell<SymbolTable>`), `ClassType::get_symbol_table` (returns `SymbolTable`), `get_members_for_class`, `get_members_for_module`, and `get_typed_dict_fields_from_dict_syntax` in `typed_dicts.rs`.
- `FunctionDetailsShared.parameters` is `Cell<P<RefCell<Vec<FunctionParam>>>>`; `SpecializedFunctionTypes.parameter_types` and `parameter_default_types` are shared arrays the same way (`[sites]`). `{ ...type.shared }` and `FunctionType.clone` copy the array reference, and `expandTypedKwargs` splices and pushes the clone's arrays, which changes the original function too (typeEvaluator.ts:2919-2923, 2944). Read with `shared.parameters.get().borrow()`; replace with `shared.parameters.set(P::new_eval(RefCell::new(v)))`.
- `OverloadedTypeDetails.priv_` is a `RefCell` (`[mutable]`): `addOverload` pushes after the type exists (types.ts:2462).
- `[items]`: `Clone` for `ClassDetailsShared`, `FunctionDetailsShared` and `TypeVarDetailsShared`, the shallow copy `{ ...type.shared }`. Write it `P::new_eval(FunctionDetailsShared::clone(&shared))`: on the `P`, `.clone()` copies the handle.

## Shared-file edits

- `crates/pyright/src/analyzer/mod.rs`: `#[cfg(test)] mod types_tests;`. The tests live in `types_tests.rs`: `--install` refuses to merge a `types.rs` that holds code at no stub marker.
- `crates/pyright/src/analyzer/typed_dicts.rs` (share H): the `class_fields` parameter of `get_typed_dict_fields_from_dict_syntax` is a `SymbolTable`, through the stubgen rerun.

## Needs from others

## Doubts

- `tests/types.test.ts` assigns closures to `synthesizeDataClassSlotsDeferred` and `synthesizeMethodsDeferred`. Those slots hold callback structs that need a `TypeEvaluator`, so the Rust test sets `has_non_empty_slots` directly and checks only `getDisjointBase`.
- A spread copy shares every array and map of the copied object; the port copies these: `ClassDetailsShared` `base_classes`, `mro`, `type_params` and `protocol_compatibility` (only `ClassType.cloneWithNewFlags`), `ClassDetailsPriv` `type_args`, `tuple_type_args` and `typed_dict_narrowed_entries`, `UnionDetailsPriv` `subtypes` and literal maps, `OverloadedDetailsPriv._overloads`, `TypeVarDetailsShared.constraints`, and the `inferredReturnType` record. No code on the check path changes one of them in place after a copy: pushes to `baseClasses`, `mro`, `constraints` and `_overloads` happen while the type is built. `isTypeSame` loses the `===` shortcuts on these arrays, which only skip a comparison that returns true.
- The singletons (`UnknownType.create()`, `AnyType.create()` and the rest) are evaluator-heap objects held in `thread_local!`s: a collector must treat them as roots. `cached` is written on them.
- `SymbolTable::new` allocates on the program heap (`P::new`). Module and class types made by the evaluator create their tables through it.
- `ClassType::is_built_in` and `FunctionType::is_built_in` take `Option<&[String]>`, so each call with a literal name allocates (`Some(&["str".to_string()])`). `&[&str]` would avoid that; it is a cross-share signature change.
- `FunctionType::clone(type_, strip_first_param, bound_to_type)` keeps its name: it is a namespace function with three parameters, not a method (porting.md "names"), and other shares call it.
