# intern notes

Intern names and string keys, so hashing and comparing a key is integer work (`docs/research/speed-levers.md`, lever 4).

## Signature changes

- `jsrt::Atom` (`crates/jsrt/src/atom.rs`): an interned string, 8 bytes, `Copy`, neither `Send` nor `Sync`. Each thread has its own table; the text is leaked. `Atom::new(&str)`, `Atom::lookup(&str)`, `atom!("literal")` (one atom per call site and thread), `as_str()`, `Deref<Target = str>`. Equality and `Hash` use the identity; it has no `Ord`.
- `TypeVarScopeId` is `Atom`. `get_scope_id_for_node` and `get_type_var_scopes_for_node` return atoms; `TypeVarDetailsPriv.scope_id`, the class, function and alias scope-id fields, `make_bound_scope_id`, the scope-id lists of the type-var transformers, `ConstraintTracker` scope ids and the transformers' pending set hold atoms.
- `TypeVarDetailsPriv.name_with_scope` is `Option<Atom>`; `make_name_with_scope` and `get_name_with_scope` return atoms; `ConstraintSet._type_var_map` and `ConstraintSolutionSet._type_var_map` are keyed by atoms.
- `ClassDetailsShared.name` and `full_name` and `ClassDetailsPriv` `alias_name` are atoms. `ClassType::is_built_in` takes `Option<&[Atom]>`: pass `Some(&[jsrt::atom!("list")])`. `derives_from_stdlib_class` takes an atom. `TypeEvaluator.type_form_special_form_diagnostic_factories` holds atom lists. Take `.to_string()` where a `String` is needed.
- `SymbolTable` is keyed by `Atom`: `get`, `has`, `set` and `delete` take `impl jsrt::AtomKey` (an atom, `&str`, `&&str` or `&String`); `for_each` passes the name as an atom; `borrow()` and `entries()` yield atoms. `IdentifierTokenDetails.value` is an atom; `NameNode::value_atom()` returns it. `Scope::look_up_symbol`, `look_up_symbol_recursive` and `add_symbol`, `TypeEvaluator::look_up_symbol_recursive` and `get_abstract_symbol_info`, and `type_utils::look_up_class_member`, `look_up_object_member` and `get_class_member_iterator` take `impl AtomKey`.
- Code-flow reference keys are atoms: `create_key_for_reference` (a name's key is its atom), `create_keys_for_reference_subexpressions`, `code_flow_expressions` and `affected_expressions` (`JsSet<Atom>`), `CodeFlowAnalyzer.flow_node_type_cache_set` keys, `get_flow_node_type_cache_for_reference` and `is_get_type_from_code_flow_pending`. `wildcard_import_reference_key()` is `WILDCARD_IMPORT_REFERENCE_KEY` as an atom.
- `CodeFlowAnalyzer.flow_node_type_cache_set` is keyed by `ReferenceKeyWithSymbolId`, the pair of reference key and target symbol id that the TypeScript formats as `${referenceKey}.${targetSymbolId}` (`None` for `'.'`); the text and the pair determine each other.

## Shared-file edits

- `crates/jsrt/src/lib.rs`: `atom` module.
- Code-flow keys: `analyzer_node_info.rs`, `binder.rs`, `code_flow_engine.rs`, `code_flow_types.rs`, `type_evaluator.rs`.
- Symbol names: `binder.rs`, `checker.rs`, `code_flow_engine.rs`, `data_classes.rs`, `decorators.rs`, `enums.rs`, `operations.rs`, `parse_nodes.rs`, `properties.rs`, `protocols.rs`, `scope.rs`, `symbol.rs`, `tokenizer.rs`, `tokenizer_types.rs`, `type_evaluator.rs`, `type_guards.rs`, `type_utils.rs`.
- Class names and `is_built_in` call sites: `checker.rs`, `code_flow_engine.rs`, `constraint_solver.rs`, `constructors.rs`, `data_classes.rs`, `decorators.rs`, `enums.rs`, `function_transform.rs`, `named_tuples.rs`, `operations.rs`, `parameter_utils.rs`, `pattern_matching.rs`, `protocols.rs`, `type_evaluator.rs`, `type_guards.rs`, `type_printer.rs`, `type_utils.rs`, `typed_dicts.rs`, `types.rs` and others the rewrite touched (`git show` lists them).
- Names with scope: `constraint_solution.rs`, `constraint_tracker.rs`, `type_evaluator.rs`, `types.rs`.
- Scope ids: `checker.rs`, `constraint_solver.rs`, `constraint_tracker.rs`, `constructor_transform.rs`, `constructors.rs`, `data_classes.rs`, `parse_tree_utils.rs`, `properties.rs`, `typed_dicts.rs`, `type_evaluator.rs`, `type_utils.rs`, `types.rs` and their tests.

## Needs from others

## Doubts

- Interned text is never freed. It grows with the distinct keys of the run (names, scope ids), as the program heap does.
