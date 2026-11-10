# solver notes

Share G of step 5 (`docs/step5-shares.md`): constraintSolver, constraintTracker, constraintSolution, operations, typePrinter, typePrinterUtils, parameterUtils, functionTransform, declarationUtils. sourceMapper, sourceMapperUtils and constructors moved to share I (801f0d7) untouched.

## Signature changes

All through `tools/stubgen`:

- `ParamAssignmentTracker::lookup_name` returns `Option<&mut ParamAssignmentInfo>` and `lookup_details` returns `&mut ParamAssignmentInfo` (`typemap.toml` `[sites]`). Callers write `argsReceived` through the returned entry (typeEvaluator.ts:12617, 12809).
- `ConstraintSolutionSet::set_type` returns nothing (`[sites]` value `"()"`, which `gen.ts` now maps to no return type). TypeScript returns the map from `Map.set`; no caller reads it.
- The six internal printers of `typePrinter.ts` take `unique_name_map: &mut UniqueNameMap` (`[sites]`). `UniqueNameMap` methods take `&mut self` because `_addIfUnique` writes `_map`, and the printers call `isUnique`.

- `ConstraintSet::clone` and `ConstraintTracker::clone` are `clone_` (`tools/stubgen/src/names.ts` `fnIdent`, applied to methods with a receiver). No caller on main called them.

## Shared-file edits

- `tools/stubgen/src/merge.ts`: an install carries a ported `thread_local!` initializer (`static X: T = init;` under its marker) like a ported function. Before, an install refused to run once any module variable was ported.
- `tools/stubgen/src/merge.ts`: a ported crate file without the blanket `#![allow(..)]` keeps it dropped on install, so clippy keeps seeing the port.
- `tools/stubgen/src/gen.ts`: a `[sites]` value of `"()"` means no return type.
- An install rewrites headers and import groups of other shares' files (rustfmt, stub header text). I revert those before each commit; only my files and the generator change.

## Needs from others

- Share E: `FunctionReturnTypeCallback::call` (typeEvaluator.ts:30313) in `type_printer.rs`.
- Share A: the typeUtils.rs bodies every file here calls.
- A decision on `synthesizeAliasDeclaration` (declarationUtils.ts:214), still `todo!()`: TypeScript builds the declaration with `node: undefined!`, and the hand-ported `Declaration.node` is a non-optional `Cell<P<ParseNode>>`. It is reached on the check path through `_addSymbolDeclInfo` (typeEvaluator.ts:23628) and the import-alias branch of `getDeclInfoForNameNode` (23785). `getNameNodeForDeclaration` tests `declaration.node === undefined` for exactly this declaration.

## Doubts

- `typePrinter.test.ts` is not ported yet. Its tests pass their own return-type callback; the Rust `FunctionReturnTypeCallback` is a struct holding a `P<TypeEvaluator>`, since the check path has one creation site. Porting the tests needs a test variant of that callback, and share A's type constructors.
- `printFunctionPartsInternal` with `PythonSyntax` prints `*args: [object Object].args`: the TypeScript interpolates the ParamSpec object. Ported as is.
- `printLiteralValue` stringifies a `SentinelLiteral` as `[object Object]`, as JavaScript would. Its callers print sentinels before reaching it.
- `operations.ts` maps its literal-math `BigInt` throws (conversion of a non-integer, `**` and `<<` past V8's size limit) to the `checked_*` methods of `jsrt::JsBigInt`; a `None` takes the TypeScript catch path. The unary `~` path has no catch in TypeScript and `expect`s instead.
- Bodies in `type_printer.rs` (a shared file) reach items by path and use fn-local `use` statements only for traits and short names; no module-level `use` line is added.
