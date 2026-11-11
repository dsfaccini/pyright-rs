# intern notes

Intern names and string keys, so hashing and comparing a key is integer work (`docs/research/speed-levers.md`, lever 4).

## Signature changes

- `jsrt::Atom` (`crates/jsrt/src/atom.rs`): an interned string, 8 bytes, `Copy`, neither `Send` nor `Sync`. Each thread has its own table; the text is leaked. `Atom::new(&str)`, `Atom::lookup(&str)`, `atom!("literal")` (one atom per call site and thread), `as_str()`, `Deref<Target = str>`. Equality and `Hash` use the identity; it has no `Ord`.
- `TypeVarScopeId` is `Atom`. `get_scope_id_for_node` and `get_type_var_scopes_for_node` return atoms; `TypeVarDetailsPriv.scope_id`, the class, function and alias scope-id fields, `make_bound_scope_id`, the scope-id lists of the type-var transformers, `ConstraintTracker` scope ids and the transformers' pending set hold atoms.

## Shared-file edits

- `crates/jsrt/src/lib.rs`: `atom` module.
- Scope ids: `checker.rs`, `constraint_solver.rs`, `constraint_tracker.rs`, `constructor_transform.rs`, `constructors.rs`, `data_classes.rs`, `parse_tree_utils.rs`, `properties.rs`, `typed_dicts.rs`, `type_evaluator.rs`, `type_utils.rs`, `types.rs` and their tests.

## Needs from others

## Doubts

- Interned text is never freed. It grows with the distinct keys of the run (names, scope ids), as the program heap does.
