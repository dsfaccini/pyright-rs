# eval-4: share E, typeEvaluator.ts:23041-30594

## Signature changes

None.

## Shared-file edits

- `crates/jsrt` (approved by the main session): `array::slice`, JS `Array.prototype.slice` on a Rust slice, with the `arrays` section of `fixtures/gen.mjs`, `fixtures/arrays.json`, `tests/arrays.rs` and a README row. `string::slice_range` became `pub(crate)` so both share one index rule. `adjustSourceParamDetailsForDestVariadic` uses it for its slices, whose end index can be negative.
- `crates/pyright/src/analyzer/type_printer.rs` (share G's file): the body of `FunctionReturnTypeCallback::call`, marker `typeEvaluator.ts:30313`, which share E owns (`docs/step5-shares.md`).

## Needs from others

None.

## Doubts

- No unit tests ported: `typeEvaluator.ts` has no test file of its own; `typeEvaluator1-8.test.ts` run sample files through the whole evaluator. Conformance comes through the corpora once all shares land.
- `get_analyzer_node_info_reader` returns `evaluator_options.node_info_reader`. TypeScript returns the `AnalyzerNodeInfoAccessor` built on that reader; its `get` and `getFileInfo` delegate to the reader, so callers see the same results.
- `getDeclInfoForStringNode` looks up a TypedDict entry by the string node's value, a `JsString`; the port converts it with `to_string_lossy`. A key with a lone surrogate would mismatch (the known gap in `docs/porting.md`, "strings").
- `getDeclInfoForNameNode`: TypeScript tests `importInfo.resolvedUris[i]`, a `Uri` object and so always truthy once the index is in range; the port drops the test.
- Where TypeScript dereferences `param.name!` and would throw on `undefined` (`kwargsParamMissing` in `assignFunction` and `validateOverrideMethodInternal`), the port unwraps and panics in the same case.
- `validateOverrideMethodInternal` formats one addendum message and discards it, as pyright does; the port keeps the call so its two `printType` calls still run.
- `assignFromUnionType`: `let remainingSrcSubtypes = sortedSrcTypes` aliases one array, so the first pass's `splice` also shrinks `sortedSrcTypes`. The port splices `sorted_src_types` and builds `remaining_src_subtypes` from it after that pass, which keeps both views equal.
- `inferTypeOfSymbolForUsage` holds the symbol's inner cache map in TypeScript; `effective_type_cache` stores inner maps by value. The port records whether an inner map existed when the call started: if so it writes into the current inner map, else it inserts a fresh map, as the TypeScript `set` does. This is exact because only `disposeEvaluator` replaces `effectiveTypeCache`, and only a call that started without an inner map replaces one.
- `callSiteReturnTypeCache` is an array that TypeScript clones of a function type share through the `priv` spread; the Rust field is a plain `Vec`. Only `getTypeOfFunction`'s result reads or writes it (typeEvaluator.ts:25376-25416), so no clone observes the difference.
- Try/catch blocks that only pop a stack and rethrow (`getTypeOfSymbolForDecls`, `getDeclaredTypeOfSymbol`) and the `finally` in `inferReturnTypeForCallSite` are ported as the try path, with every push and pop where the TypeScript has it.
- Five helper agents ported 24095-30161 in parallel from copies of this branch; each body was compiled in its copy and again here.
