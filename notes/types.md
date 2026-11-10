# types notes

Share A of step 5: `analyzer/types.ts` and `analyzer/typeUtils.ts` (`docs/step5-shares.md`).

## Signature changes

- `analyzer/symbol.ts:SymbolTable` maps to the `SymbolTable` handle of `analyzer/symbol.rs` (`[types]` in `typemap.toml`). A module type holds its scope's table (typeEvaluator.ts:24280), `isTypeSame` compares module tables with `===` (types.ts:3766), and spread copies of a type share the table. Changed: `ModuleDetailsPriv.fields` and `loader_fields`, `ModuleType::create`, `ClassDetailsShared.fields` (`Cell<SymbolTable>`), `ClassType::get_symbol_table` (returns `SymbolTable`), `get_members_for_class`, `get_members_for_module`, and `get_typed_dict_fields_from_dict_syntax` in `typed_dicts.rs`.
- `FunctionDetailsShared.parameters` is `Cell<P<RefCell<Vec<FunctionParam>>>>`; `SpecializedFunctionTypes.parameter_types` and `parameter_default_types` are shared arrays the same way (`[sites]`). `{ ...type.shared }` and `FunctionType.clone` copy the array reference, and `expandTypedKwargs` splices and pushes the clone's arrays, which changes the original function too (typeEvaluator.ts:2919-2923, 2944). Read with `shared.parameters.get().borrow()`; replace with `shared.parameters.set(P::new_eval(RefCell::new(v)))`.
- `OverloadedTypeDetails.priv_` is a `RefCell` (`[mutable]`): `addOverload` pushes after the type exists (types.ts:2462).

## Shared-file edits

- `crates/pyright/src/analyzer/typed_dicts.rs` (share H): the `class_fields` parameter of `get_typed_dict_fields_from_dict_syntax` is a `SymbolTable`, through the stubgen rerun.

## Needs from others

## Doubts

- `SymbolTable::new` allocates on the program heap (`P::new`). Module and class types made by the evaluator create their tables through it.
