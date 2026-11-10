# checker notes

The checker round of step 5: `analyzer/checker.ts`, `analyzer/deprecatedSymbols.ts`, and the hooks `run_checker` (`analyzer/source_file.rs`) and `dump_types` (`oracle/types.rs`).
checker.ts was ported in four ranges, 1-1864, 1865-3923, 3924-5728 and 5729-7900, each against the same generated signatures.

## Signature changes

- `Checker::new` takes `node_info_reader: P<dyn AnalyzerNodeInfoReader>` (was `&dyn`): the walker base stores it (checker.ts:244). `typemap.toml` `[sites]` `Checker.constructor.nodeInfoReader`.
- `FunctionReturnTypeCallback` (typePrinter.ts:104) is a trait, held as `P<dyn FunctionReturnTypeCallback>` in every parameter and in `UniqueNameMap._return_type_callback` (was a one-site stored-callback struct holding the evaluator). `TypeEvaluator` implements it at typeEvaluator.ts:30313; the oracle and the typePrinter tests have their own implementations. `typemap.toml` `[types]` and `[items]`; `tools/stubgen/src/emit.ts` emits nothing at a function type alias that `[types]` maps. Recorded in `docs/evaluator-design.md` (stored callbacks) and `docs/step5-shares.md`. Approved by the main session, 2026-11-10.
- `SourceFile::check` takes pyright's parameters: `import_lookup`, `import_resolver`, `evaluator`, `dependent_files`, `node_info_reader`.
- `stubgen --install` refuses on main: `type_evaluator.rs` holds a nested function marker (`typeEvaluator.ts:24272`, `apply_loader_actions_to_module_type`) that the stub lacks. The changes above went into `crates/` by hand, identical to the generated stubs (checked: every `checker.rs` signature matches `tools/stubgen/out`).

## Shared-file edits

- `analyzer/program.rs`: the evaluator. `Program` gains `evaluator`, `evaluator()` (program.ts:217) and `create_new_evaluator` (program.ts:1974), called where pyright calls it: the constructor, `set_config_options`, `set_import_resolver`. `check_types` passes pyright's arguments to `SourceFile::check`; `dependentFiles` is `None` (`_checkDependentFiles` returns undefined outside notebooks).
- `analyzer/source_file.rs`: `check`'s parameters, the "Parse results not available" assert, and `run_checker`.
- `analyzer/import_statement_utils.rs`: `getRelativeModuleName` (`_getImportResult`), and `getTopLevelImports` with `ImportStatement`, `ImportStatements`, `_getTopLevelImports`, `_processImportNode`, `_processImportFromNode` and `formatModuleName` (`_reportDuplicateImports`). A statement sits in both `orderedImports` and `mapByFilePath`, so it is an `Rc`.
- `analyzer/type_printer.rs`, `analyzer/type_evaluator.rs` (the three share E `printType` sites), `analyzer/type_printer_tests.rs`: the callback trait above. The tests now pass typePrinter.test.ts's own callback, `declaredReturnType ?? UnknownType.create(true)`, instead of an evaluator.
- `oracle/types.rs`: `dump_types`.

## Needs from others

- Nothing blocks the checker bodies. They call functions other shares have not filled yet (some `type_utils`, `typed_dicts`, `declaration_utils::get_name_node_for_declaration`, `evaluate_type_for_subnode`); those panic at run time until they land.

## Doubts

- The `try`/`catch` in `SourceFile.check` is not ported: it turns a crash into an `internalTypeCheckingError` diagnostic (porting.md, "exceptions").
- `Commands.unusedImport` is the literal `"pyright.unusedImport"` (commands.ts:14): no `commands` module is ported.
- For a one-part `import x` without an alias, `_conditionallyReportUnusedDeclaration` adds `reportUnusedImport` once directly and once at its end, as pyright does; the sink's dedup decides what survives.
- `_reportUnusedDunderAllSymbols`: a `__all__` string with a lone surrogate names no symbol, and its message prints U+FFFD (the known `String` gap).
- `_walkStatementsAndReportUnreachable` reads `firstToken` through a local match over the ten statement kinds; `Error` uses its own range, as pyright does.
- `_validateSymbolTables`: a type parameter without a symbol returns from the whole function, as in pyright.
- `_validateDataClassPostInit` keeps pyright's quirk: `getParamDefaultType` gets the index into the virtual parameter list, not `params[i].index`. Two unnamed parameters compare equal (`undefined === undefined`).
- Where pyright reads `undefined` and then crashes (`node.d.arguments[index]` in `_validateEnumClassOverride`, `getDeclarations()[0]`, a declaration node assumed to be a `FunctionNode`, `node.d.params[kwargsIndex]` in `visitFunction`), the port panics in the same cases.
- An empty `typeVarScopeId` or parameter name is falsy in pyright; `_validateBaseClassOverride` and `_validateClsSelfParamType` filter them out to match.
- `_isTypeSupportedTypeForIsInstance` never reads `isInstanceCheck`, in pyright too.
- `ClassDetailsPriv.tuple_type_args` is about to become a shared `P<RefCell<Vec<..>>>` (flow); `visitAssert` and `visitIndex` read it and follow that change at the rebase.
- `dump_types` reads the evaluator's `type_cache` and `evaluator_options` fields directly, as the fork's `oracleReadTypeCacheEntry` does inside `createTypeEvaluator`.
