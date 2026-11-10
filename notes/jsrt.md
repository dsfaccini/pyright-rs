# jsrt notes

Step 2 of `PLAN.md`: JavaScript semantics in `crates/jsrt`.

## Signature changes

None. `arena.rs` (`P<T>`) and `collections.rs` (`JsMap`, `JsSet`) keep their API.

## Shared-file edits

- `Cargo.lock`: dependencies added with `cargo add -p jsrt`.

## Needs from others

None.

## Doubts

None.

## Dependencies

- `unicode-normalization = "=0.1.25"`: NFC/NFD/NFKC/NFKD. Pinned: 0.1.25 carries Unicode 17, the version of Node 26's ICU 78.3. 0.1.26 carries Unicode 18 and fails `tests/normalize.rs` against Node.
- `num-bigint`, `num-traits`: JS `bigint`. Exact arithmetic; `/` truncates and `%` takes the dividend's sign, as JS does. `JsBigInt` adds V8's 2^30-bit limit on top.
- `ryu`: shortest round-trip digits for `String(n)`. Rust's `{:e}` breaks exact ties upward (2^-25 prints `...313e-8`); V8's Dragonbox and the spec break them to even (`...312e-8`), and so does Ryu.
- `serde_json` (dev): reads the fixtures.

## Unicode data

- Case mapping uses Rust std's `char::to_lowercase`/`to_uppercase`, which are Unicode 17 in Rust 1.99, plus ICU's final-sigma rule. `src/string/case_tables.rs` (Cased, Case_Ignorable) is generated from Node's regex property escapes by `fixtures/gen.mjs`.
- When the Rust toolchain or Node moves to a new Unicode version, regenerate the fixtures and rerun `cargo test -p jsrt`: the per-code-point tests show any drift.

## Number calls on the check path

Found by grepping the import closure of `pyright.ts` for `parseInt`, `parseFloat`, `Number(`, `BigInt`, `bigint`, `toString(`, `toFixed`, `toPrecision` and `Math.`. `toFixed`, `toPrecision` and `toExponential` do not occur.

| call site | JS | jsrt |
|---|---|---|
| `parser/tokenizer.ts` `_tryNumber` | `parseInt(text, 2/8/16/10)`, `BigInt(text)`, `parseFloat(text)`, bigint vs `Number.MIN/MAX_SAFE_INTEGER` | `number::parse_int`, `number::parse_bigint`, `number::parse_float`, `JsBigInt: PartialOrd<f64>` |
| `parser/tokenizer.ts` `_setIndent` | `Math.round` | `number::math_round` |
| `parser/parser.ts` invalid token message | `charCode.toString(16)` | `number::number_to_string_radix` |
| `analyzer/typePrinterUtils.ts` `printBytesLiteral` | `nibble.toString(16)` | `number::number_to_string_radix` |
| `analyzer/parseTreeUtils.ts` `printExpression` | `value.toString()` on `number \| bigint` | `NumberValue::to_js_string` |
| `analyzer/typePrinter.ts` `printLiteralValue` | `literalValue.toString()` | `NumberValue::to_js_string` |
| `analyzer/operations.ts` literal math | bigint `+ - * / % ** << >> & \| ^`, unary `-`, `Number(bigint)`, `BigInt(number)`, try/catch on RangeError | `JsBigInt::checked_*`, `& \| ^ ! -`, `to_f64`, `from_f64` |
| `analyzer/staticExpressions.ts`, `patternMatching.ts`, `typeEvaluator.ts` | `=== BigInt(0)`, `=== BigInt(n)` | `JsBigInt::from`, `==` |
| `analyzer/types.ts` `literalIntMap` | `Map<bigint \| number>` keys | `NumberValue` (`Eq`/`Hash` are SameValueZero) |
| `common/pythonVersion.ts` `fromString` | `parseInt(s, 10)` | `number::parse_int` |
| `common/core.ts` `getEnumNames` | `Number(key)` | `number::string_to_number` |
| `localization/localize.ts` `format`, template literals | `n.toString()`, `${n}` | `number::number_to_string` |

Not on the `--outputjson` path: `cacheManager.ts`, `timing.ts`, `crypto.ts`, and `pyright.ts`'s `parseInt` of CLI worker arguments and `Math.round` of the completeness score.

An integer below 1e21 prints the same through Rust's `Display` as through `number_to_string`.
