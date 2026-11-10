# eval-3: share D, typeEvaluator.ts:15242-23040

## Signature changes

None.

## Shared-file edits

None yet. Share D's three stored callbacks in `types.rs` (`typeEvaluator.ts:19573`, `19577`, `19586`) are share D's markers.

## Needs from others

- tools/stubgen owner: `stubgen --install` rewrites more than the stubs of unported functions. Run on main at 980e3e3 it put back the stub header (`//! Stubs for ...`, `#![allow(...)]`, split `use` lines) on fully ported files (`sentinel.rs`, `symbol_utils.rs`, `type_complexity.rs`, `type_walker.rs`), and it sorts `use crate::{analyzer, common, parser};` below the other imports where main keeps it first. I reverted those changes by hand each time.

## Doubts

- `invalidateTypeCacheIfCanceled` (common/cancellationUtils.ts) is not ported: it only tags a cancellation exception, and the CLI check never cancels. `getTypeOfLambdaWithExpectedType` runs its callback inline.
- `createLiteralType` joins the string nodes' UTF-16 units before decoding to `LiteralValue::String`, so a surrogate pair split across two string nodes decodes as one character, as JavaScript's `join` gives. A lone surrogate still becomes U+FFFD (the known gap in `docs/porting.md`, "strings").
- `getTypeOfYieldFrom` keeps pyright's `generatorTypeArgs.length >= 2 ? generatorTypeArgs[2] : ...`: with exactly two type args the callback returns undefined and `mapSubtypes` drops the subtype.
- `getTypeOfLambdaWithExpectedType` compares `!param.d.name === !expectedParam.param.name`; an expected parameter named `""` counts as unnamed, as in JavaScript.
- Where TypeScript casts a solved type with `as ClassType` and then reads `priv.typeArgs`, the port uses `ClassType::cast` and treats a non-class like a class without type args: TypeScript reads `undefined` there.
- No unit tests ported: `typeEvaluator.ts` has no test file of its own.
