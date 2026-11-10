# jsrt notes

Step 2 of `PLAN.md`: JavaScript semantics in `crates/jsrt`.

## Signature changes

None. `arena.rs` (`P<T>`) and `collections.rs` (`JsMap`, `JsSet`) keep their API.

## Shared-file edits

- `Cargo.lock`: dependencies added with `cargo add -p jsrt`.

## Needs from others

- infra: `crates/pyright/src/common/core.rs` defines `is_js_whitespace_char` with `as` casts. `jsrt::string::is_js_whitespace_char` is the same predicate, tested against node; the porting contract puts Rust-only helpers in `jsrt`.

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

## Sorting

`sort::sort_by` ports V8 14.6's `third_party/v8/builtins/array-sort.tq` (Node v26.13.0's `deps/v8`). `tests/sort.rs` checks 1,872 seeded cases (8 input shapes, 9 comparators including random results, rock-paper-scissors, "two literals return 1" and NaN, sizes 0 to 3,001) for the same call count, call sequence hash and permutation as node.

Sort calls on the check path: comparator sorts in `analyzer/types.ts` (`combineTypes` and `sortTypes`; the union comparator returns 1 for two literals), `analyzer/typeUtils.ts`, `analyzer/program.ts`, `analyzer/parseTreeUtils.ts`, `analyzer/pythonPathUtils.ts`, `analyzer/importStatementUtils.ts`, `common/uri/uriUtils.ts`, `common/extraPathGlob.ts`, `common/collectionUtils.ts` (`cloneAndSort`, `stableSort`), `common/fullAccessHost.ts`, and `pyright.ts` (diagnostics by `compareDiagnostics`). One default sort: `analyzer/typeshedInfoProvider.ts`, on strings. `localeCompare` occurs only in `typeStubGeneration.ts` and `tracePrinter.ts`, which a check does not run.

## Regexes in the milestone 1 files

Found by grepping the TypeScript files behind `crates/pyright/src` for regex literals, `RegExp`, `.match(`, `.test(`, `.exec(`, `.search(` and regex `split`/`replace`. None of them has the `u`, `s`, `m` or `y` flag, and none is a `g` regex reused with `.test()`, so `lastIndex` state never matters.

Features used: anchors `^ $`, classes with ASCII ranges, negated classes, `\s`, `\w` (also inside classes), `\b`, `.`, `?` `*` `+` `{0,2}` `{2,}` and lazy `*?`, groups and non-capturing groups, alternation, one lookahead `(?=...)`, the `i` flag, the `g` flag with `replace` and `split`, and patterns built at run time with `new RegExp` (path wildcards, localized message keys, workspace names).

Where the `regex` crate differs observably, `jsrt::js_regex` has the JS spelling (tested against node on every BMP character):

| JS (no `u` flag) | `regex` crate default | jsrt |
|---|---|---|
| `\s`: JS whitespace set, has U+FEFF | Unicode White_Space: has U+0085, lacks U+FEFF | `js_regex::WHITESPACE`, `WHITESPACE_IN_CLASS`, `string::is_js_whitespace_char` |
| `\w`, `\d`: ASCII | Unicode letters and digits | `js_regex::WORD`, `WORD_IN_CLASS`, `DIGIT` |
| `\b`: ASCII word boundary | Unicode word boundary | `js_regex::WORD_BOUNDARY` |
| `.`: stops at `\n \r U+2028 U+2029` | stops at `\n` only | `js_regex::DOT` |
| `/i` with ASCII letters: ASCII folding only | `(?i)` folds `ſ` to `s`, U+212A to `k` | wrap the letters in `(?i-u:...)` |
| `replace(re, string)` expands `$$ $& $\` $'` | `Regex::replace` expands `$name` | `JsStr::js_replace_all` when `re` is a literal |

Not covered by a helper:
- A non-`u` JS regex matches UTF-16 code units: `.` and `[^/]` consume half of an astral character, so `/^a?b$/` with `?` as `[^/]` (a `?` path wildcard) does not match `a😀b` in JS but does in Rust. Only file specs with `?` and astral file names reach this.
- Lookahead has no `regex` crate equivalent. `baseUri.ts` `containsExtension` splits with `/(?=\.)/`: pieces start before every `.` except at index 0 (`".a.b..c"` gives `[".a", ".b", ".", ".c"]`). `envVarUtils.ts` replaces `/(?:^|\/)~(?=\/)/g` with the home directory: the match includes the leading `/`, and the home directory goes through `$`-pattern substitution.
- JS `/i` with non-ASCII letters (path regexes on a case-insensitive file system, `extraPathGlob.ts` segments) canonicalizes by simple uppercase; `regex` folds by simple case folding. They differ for characters like U+1E9E and U+212B. A case-sensitive file system, as on Linux, never builds such a regex.

Call sites and how to port them:

| call site | JS | port |
|---|---|---|
| `analyzer/pyTypedUtils.ts` | `/partial\n/`, `/partial\r\n/` | `str::contains` |
| `analyzer/pythonPathUtils.ts` | `split(/\r?\n/)`, `/^import\s/` | split on `\n`, then strip one trailing `\r` from every piece but the last; `starts_with("import")` then `is_js_whitespace_char` |
| `analyzer/docStringUtils.ts` | `/\r/g`, `/\t/g` replaced by `''`, 8 spaces; `/^:returns?:\s*(.*)$/i`, `/^@returns?\b:?\s*(.*)$/i`, `/^returns?:$/i` | `str::replace`; `(?i-u:...)` around the letters, `WHITESPACE`, `WORD_BOUNDARY`, `DOT` |
| `common/uri/baseUri.ts` | `/\\/g`; `split(/(?=\.)/g)` | `str::replace`; by hand (above) |
| `common/uri/uri.ts` | `/^\/[a-zA-Z]:\//`, `/\//g`, `/^[a-zA-Z]:\\?/`, `/^[a-zA-Z][a-zA-Z0-9+.-]*:\/?\/?/` | same pattern in `regex` (ASCII only) |
| `analyzer/service.ts` | `/^[a-zA-Z]{2,}[\w+.-]*:\/\/?/` | `\w` as `WORD_IN_CLASS` |
| `analyzer/symbolNameUtils.ts` | `/^[A-Z0-9_]+$/`, `/^[_]+$/`, `/^_{0,2}[A-Z][A-Za-z0-9_]+$/` | same pattern (ASCII only) |
| `analyzer/importResolver.ts` | `/[.-]/` | same pattern |
| `common/core.ts` | `/^\s*$/` | `is_js_whitespace_char` |
| `common/stringUtils.ts` `escapeRegExp` | `replace(/[\\^$.*+?()[\]{}\|]/g, '\\$&')` | `regex::escape` when the result feeds a `regex` pattern |
| `common/envVarUtils.ts` | `/\$\{workspaceFolder\}/g`, `${workspaceFolder:<escaped>}`, `/\$\{env:HOME\}/g` and friends, replaced by paths | `js_replace_all` with the literal text: the paths go through `$` substitution |
| `common/pathUtils.ts`, `common/uri/uriUtils.ts` wildcard specs | `[^\w\s<sep>]` escape, `[^/]`, `(/[^/][^/]*)*?`, `/\.pyi?$/`, `/[*?]/`, `i` on case-insensitive file systems | `WORD_IN_CLASS`, `WHITESPACE_IN_CLASS`; already ported on main by infra |
| `common/extraPathGlob.ts` | `new RegExp('^' + segment + '$', i?)` | as above |
| `common/realFileSystem.ts` | `/[^\\/]\.(?:egg\|zip\|jar)[\\/]/` | same pattern |
| `localization/localize.ts` `format` | `replace(new RegExp('{' + key + '}', 'g'), value)` | `js_replace_all("{key}", value)`: `{` without digits is a literal in a non-`u` regex |
| `parser/parser.ts` | `/^(\s*#\s*type:\s*)([^\r\n]*)/`, `/^ignore(\s\|\[\|$)/` | `WHITESPACE` |
| `pyright.ts` | `replace(/[\r\n]/g, ' ')` on stdin file lists | replace each `\r` and `\n` by a space |
| `common/debug.ts` `getFunctionName` | `/^function\s+([\w$]+)\s*\(/` | not on the output path |
