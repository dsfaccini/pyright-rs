# mapper notes

Share I of step 5 (`docs/step5-shares.md`): sourceMapper, sourceMapperUtils, constructors, and `program.ts:1439-1440`.

## Signature changes

None.

## Shared-file edits

- `crates/pyright/src/analyzer/mod.rs`: registers `source_mapper_utils_tests` under `#[cfg(test)]`.

## Needs from others

- Every `constructors.rs` body calls `type_utils.rs` (B), `type_evaluator.rs` (B to E) and `constraint_solver.rs` (G) items that may still be `todo!()`.

## Doubts

- `validate_new_and_init_methods` keeps the TypeScript's last write to `validatedArgExpressions`, which nothing reads; `#[allow(unused_assignments)]` covers it.
- `buildImportTree` drops the `token.isCancellationRequested` checks: `CancellationToken` is `()` on the check path. The `Canceled` test is `#[ignore]`d for that reason.
- The `SourceMapper` class and the `ShadowFileBinder` and `BoundSourceGetter` callbacks are still `todo!()`. Only the language server builds a `SourceMapper` (`Program.getSourceMapper`, called from `languageService/`, `typeServer/` and `common/extensibility.ts`). Its bodies call `ImportResolver.getSourceFilesFromStub` and `getTypeshedStdLibPath`, `Program.bindShadowFile` and `addInterimFile`, and `Uri.toString`, which `import_resolver.rs` and `program.rs` leave unported as LSP-only. Decision pending with the main session.
