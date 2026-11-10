# eval-3: share D, typeEvaluator.ts:15242-23040

## Signature changes

None.

## Shared-file edits

None yet. Share D's three stored callbacks in `types.rs` (`typeEvaluator.ts:19573`, `19577`, `19586`) are share D's markers.

## Needs from others

None open.

## Doubts

- `invalidateTypeCacheIfCanceled` (common/cancellationUtils.ts) is not ported: it only tags a cancellation exception, and the CLI check never cancels. `getTypeOfLambdaWithExpectedType` runs its callback inline.
- `createLiteralType` joins the string nodes' UTF-16 units before decoding to `LiteralValue::String`, so a surrogate pair split across two string nodes decodes as one character, as JavaScript's `join` gives. A lone surrogate still becomes U+FFFD (the known gap in `docs/porting.md`, "strings").
- `getTypeOfYieldFrom` keeps pyright's `generatorTypeArgs.length >= 2 ? generatorTypeArgs[2] : ...`: with exactly two type args the callback returns undefined and `mapSubtypes` drops the subtype.
- `getTypeOfLambdaWithExpectedType` compares `!param.d.name === !expectedParam.param.name`; an expected parameter named `""` counts as unnamed, as in JavaScript.
- Where TypeScript casts a solved type with `as ClassType` and then reads `priv.typeArgs`, the port uses `ClassType::cast` and treats a non-class like a class without type args: TypeScript reads `undefined` there.
- No unit tests ported: `typeEvaluator.ts` has no test file of its own.
- `evaluateTypesForStatement` ports `fail('Unexpected statement')` as `panic!`.
- `createSpecializedClassType` indexes `typeParams[index]` for each type arg below `typeArgCount`. A pseudo-generic class that is still partially evaluated keeps `typeParams` empty while `typeArgCount` is not clamped, so the index runs past the end: TypeScript passes `undefined` to `applyTypeArgToTypeVar` (harmless only when the arg is Any or Unknown), the port panics.
