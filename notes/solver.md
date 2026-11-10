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
- `analyzer/declaration.rs`: `undefined_declaration_node()`, JavaScript `undefined` as a `Declaration.node` (`synthesizeAliasDeclaration`'s `node: undefined!`). One node, identity-distinct from every real node, with id 0 so it takes no id from the parser's counter. A TypeScript test `decl.node === undefined` or `!decl.node` ports as `decl.node.get() == undefined_declaration_node()`. `getNameNodeForDeclaration` (declarationUtils.ts:153) is the one such site in share G. The others are unported: sourceMapper.ts:843 and 1038 (`!decl.node`, share I) and checker.ts:4566 (`primaryDeclaration.node && ...`).
- `analyzer/mod.rs`: the `type_printer_tests` test module.
- An install rewrites headers and import groups of other shares' files (rustfmt, stub header text). I revert those before each commit; only my files and the generator change.

## Needs from others

None. Share E ported `FunctionReturnTypeCallback::call` and share B all of typeUtils.

## Doubts

- `typePrinter.test.ts` (`analyzer/type_printer_tests.rs`) passes its own return-type callback, `declaredReturnType ?? UnknownType.create(true)`. The Rust `FunctionReturnTypeCallback` has one form, a struct holding a `P<TypeEvaluator>`, since the check path has one creation site. The tests build an evaluator with an empty import lookup and node-info store and use that callback: it returns the declared return type, and for the one test function without one, inference gives `Unknown`, which prints the same.
- `printFunctionPartsInternal` with `PythonSyntax` prints `*args: [object Object].args`: the TypeScript interpolates the ParamSpec object. Ported as is.
- `printLiteralValue` stringifies a `SentinelLiteral` as `[object Object]`, as JavaScript would. Its callers print sentinels before reaching it.
- `operations.ts` maps its literal-math `BigInt` throws (conversion of a non-integer, `**` and `<<` past V8's size limit) to the `checked_*` methods of `jsrt::JsBigInt`; a `None` takes the TypeScript catch path. The unary `~` path has no catch in TypeScript and `expect`s instead.
- Bodies in `type_printer.rs` (a shared file) reach items by path and use fn-local `use` statements only for traits and short names; no module-level `use` line is added.
