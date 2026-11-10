# skeleton notes

The step 5 skeleton: `tools/stubgen` run `evaluator` installed into `crates/pyright/src`, and `docs/step5-shares.md`.

## Signature changes

- `jsrt::P<T>` accepts unsized `T`, so a stored trait object is a `P<dyn Trait>`; `P::to_dyn` makes one from the implementing struct's `P`. Equality and hashing compare addresses, as before for sized `T`.
- `ParseTreeWalker::annotation_node_info` returns `Option<P<dyn AnalyzerNodeInfoReader>>` (was `Option<&'static dyn ..>`): the checker stores it, and no generated struct stores a `&'static`.
- `analyzer/types.rs`: the binder's placeholder `Type` is the generated family root. `TypeVarScopeId` stays `String`.
- `analyzer/declaration_utils.rs`: the generated stub, with `has_type_for_declaration` and `are_declarations_same` kept as ported. They take `&Declaration`; the generated functions of the family take `P<Declaration>`.
- `analyzer/source_file.rs`: `checker_enabled()` reads `PYRIGHT_RS_CHECKER=1`. `run_checker` returns an empty list without it, and reaches `todo!()` with it.
- `oracle/types.rs`: `dump_types(program, root, printed)`, a `todo!()`; `oracle_dump_final` calls it only with `checker_enabled()`, the stand-in for the fork's `program.evaluator` check.

## Shared-file edits

- `crates/jsrt/src/arena.rs`: `P<T: ?Sized>`, `P::to_dyn`, address-based `PartialEq` and `Hash`, and a test.
- `crates/pyright/src/analyzer/parse_tree_walker.rs`: the accessor above, and `reader.get()` in `get_string_annotation`.
- `crates/pyright/src/analyzer/source_file.rs`: `checker_enabled` and the `run_checker` hook.
- `crates/pyright/src/oracle/dumper.rs`, `crates/pyright/src/oracle/types.rs`: the types stage hook.
- `crates/pyright/src/analyzer/mod.rs`: one `pub mod` line per new module.

## Needs from others

- `common/diagnostic.rs` owner: `DiagnosticAddendum::new` allocates with `P::new`, and `create_addendum` calls it. The evaluator creates most addenda, so those land on the program heap. They need a `P::new_eval` constructor.
- Corpus runs: neither `target/release` holds `typeshed-fallback`, so `pyright-rs` resolves the stdlib to the venv's Python unless `PYRIGHT_RS_MODULE_PATH` names a directory that holds it. The milestone 1 check here used `PYRIGHT_RS_MODULE_PATH=~/pyright-oracle/packages/pyright/dist`, the typeshed the reference used.

## Doubts

- The handle rule is per type, so it errs toward `P`. `ConstraintTracker` and `ConstraintSet` stay handles (`docs/evaluator-design.md`, section 3, "objects"), and their clones fill the evaluator heap until a collector exists.
- A stored callback copies the variables its closure captures. Where the TypeScript writes a captured `let` after making the closure, the port needs a shared cell; the struct marks each such field.
- Every stub file allows `clippy::all` and unused code. A file whose last `todo!()` is gone should drop that line so clippy sees the port.
- `PYRIGHT_RS_CHECKER` defaults off so the milestone 1 check runs unchanged. Once the checker works, the switch goes and the checker always runs.
