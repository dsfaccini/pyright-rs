# guards notes

Share J of step 5: `typeGuards.ts:2068-3024` (`docs/step5-shares.md`), the narrowing helpers from `narrowTypeForTupleLength` to `isMatchingExpressionOrWalrusRhs`, in `crates/pyright/src/analyzer/type_guards.rs`.

## Signature changes

None.

## Shared-file edits

None. `type_guards.rs` is shared with share F (`typeGuards.ts:1-2067`). The ported bodies add no module-level `use` line: each body imports what it needs with a `use` inside the function, so F's import block and mine cannot collide at merge.

## Needs from others

None. Every function these bodies call is ported on `main` (`typeUtils.ts` by share B, `getInnermostNewTypeBaseInstance` by share F, the evaluator methods by shares B to E).

## Tests

pyright has no unit test for `typeGuards.ts` (`packages/pyright-internal/src/tests/` holds none), so this share ports none. Each function needs a live evaluator and a bound program, so a unit test would rebuild most of the check. Conformance comes from the corpora once all shares land.

## Doubts

- `literalValue as string` (`narrowTypeForTypedDictKey`, `narrowTypeForDiscriminatedDictEntryComparison`): every caller in `getTypeNarrowingCallback` passes a `str` literal (`ClassType.isBuiltIn(.., 'str')` with `isLiteralType`). The port reads `LiteralValue::String` and treats any other value as unreachable. JavaScript would look up a non-string key and find nothing.
- `tupleTypeArgs[indexValue]?.type` (`narrowTypeForDiscriminatedTupleComparison`): a fractional index reads `undefined` in JavaScript, so the port reads no entry when `index_value` has a fraction. Python int literals never have one.
- `lengthValue` and `elementsToAdd` stay `f64`, and the expansion loops count in `f64`, as the TypeScript does.
