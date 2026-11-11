# jsrt

What pyright gets from JavaScript and V8: object references, ordered collections, and JS string, number and sort semantics. Call these where `docs/porting.md` says JavaScript and Rust differ.

Every helper is tested against Node 26 through fixtures in `fixtures/`. Regenerate them with `node crates/jsrt/fixtures/gen.mjs` (Node 26 on PATH), then run `cargo test -p jsrt`.

## objects and collections

| JavaScript | Rust |
|---|---|
| object reference, `===` on objects | `P<T>` (`P::new`, `==` is identity) |
| `Map`, `Set` | `JsMap`, `JsSet` (delete with `shift_remove`) |
| lookup-only map or set | `FxHashMap`, `FxHashSet` (never iterate) |
| `Map` that most owners leave empty and the rest fill with a few entries | `LazyJsMap` (one pointer while empty, searched linearly) |
| `Set` that holds a few elements at a time | `SmallJsSet` (searched linearly) |

## arrays

| JavaScript | Rust |
|---|---|
| `a.slice(start, end)` | `array::slice(&a, start, end)` (`None` is an omitted `end`; borrows, so `.to_vec()` for the copy) |

## strings

`JsString` is an immutable UTF-16 string that may hold lone surrogates. Build it in a `Vec<u16>`, then `JsString::from(vec)`; it also converts from `&str`, `String` and `&[u16]`, and derefs to `[u16]`. `to_string_lossy()` turns a lone surrogate into U+FFFD. Its `Ord` is JS `<`. `utf16!("text")` is a `&'static [u16]` built at compile time.

The trait `JsStr` gives JS results on `str` and `[u16]` (and so on `JsString`). Indices and lengths are UTF-16 code units, as `i32`. An omitted JS argument is `None`.

| JavaScript | Rust |
|---|---|
| `s.length` | `s.js_len()` |
| `s.charCodeAt(i)` / `s.codePointAt(i)` | `s.js_char_code_at(i)` / `s.js_code_point_at(i)` (`None` is `NaN` / `undefined`) |
| `s.slice(a, b)`, `s.substring(a, b)`, `s.substr(a, n)` | `s.js_slice(a, b)`, `s.js_substring(a, b)`, `s.js_substr(a, n)` |
| `a < b`, `a > b` on strings | `a.js_cmp(b)` |
| `s.trim()`, `s.trimStart()`, `s.trimEnd()` | `s.js_trim()`, `s.js_trim_start()`, `s.js_trim_end()` |
| `s.toLowerCase()`, `s.toUpperCase()` | `s.js_to_lower_case()`, `s.js_to_upper_case()` |
| `s.split(sep, limit)` | `s.js_split(sep, limit)` |
| `s.indexOf(t, p)`, `s.lastIndexOf(t, p)`, `s.includes(t, p)` | `s.js_index_of(t, p)`, `s.js_last_index_of(t, p)`, `s.js_includes(t, p)` |
| `s.startsWith(t, p)`, `s.endsWith(t, p)` | `s.js_starts_with(t, p)`, `s.js_ends_with(t, p)` |
| `s.normalize('NFKC')` | `s.js_normalize(NormalizationForm::Nfkc)` |
| `JSON.stringify(s)` | `s.js_json_stringify()` |
| `s.replace(t, r)`, `s.replaceAll(t, r)` with a string `t` | `s.js_replace(t, r)`, `s.js_replace_all(t, r)` (expand `` $$ $& $` $' ``) |
| `str.replace(new RegExp(literal, 'g'), r)` | `s.js_replace_all(literal, r)` when the regex matches only the literal text |
| `String.fromCharCode(n)` | `string::from_char_code(n)` |
| WhiteSpace or LineTerminator test | `string::is_js_whitespace(unit)` |

On `str`, `js_slice`, `js_substring`, `js_substr` and `js_split` return `Cow<str>`: a surrogate pair split in half becomes U+FFFD, which keeps the UTF-16 length. On `[u16]` they return `&[u16]`, exact.

## numbers

| JavaScript | Rust |
|---|---|
| `String(x)`, `x.toString()`, `${x}` for a number | `number::number_to_string(x)` |
| `` s += `${n}` `` for an integer, without allocating | `number::push_integer(&mut s, n)` |
| `x.toString(radix)` | `number::number_to_string_radix(x, radix)` |
| `parseInt(s, radix)` | `number::parse_int(s, radix)` (0 for an omitted radix) |
| `parseFloat(s)` | `number::parse_float(s)` |
| `Number(s)` for a string | `number::string_to_number(s)` |
| `Math.round(x)` | `number::math_round(x)` |
| `number \| bigint` | `NumberValue` (`Eq` and `Hash` are SameValueZero, as for `Map` keys) |
| `BigInt(s)` | `number::parse_bigint(s)` (`None` is a SyntaxError) |
| `BigInt(x)`, `Number(b)` | `JsBigInt::from_f64(x)` (`None` is a RangeError), `b.to_f64()` |
| `b.toString()`, `b.toString(radix)` | `b.to_js_string()`, `b.to_string_radix(radix)` |
| `a + b`, `-`, `*`, `/`, `%`, `**`, `<<`, `>>` on bigints | `a.checked_add(&b)` and so on (`None` where V8 throws: division by zero, negative exponent, results past 2^30 bits) |
| `-b`, `~b`, `a & b`, `a \| b`, `a ^ b` | `-&b`, `!&b`, `&a & &b`, `&a \| &b`, `&a ^ &b` |
| `a < b` between a bigint and a number | `JsBigInt: PartialOrd<f64>`, `PartialEq<f64>` |

An integer below 1e21 prints the same through Rust's `Display` as through `number_to_string`.

## sorting

| JavaScript | Rust |
|---|---|
| `array.sort(compare)` | `sort::sort_by(&mut v, compare)` |
| `strings.sort()` | `sort::sort_strings(&mut v)` |

`sort_by` ports V8's TimSort and makes the same comparator calls in the same order, so a comparator that is not a total order, or has side effects, behaves as in pyright. The comparator may return `f64` (NaN counts as 0), `i32`, `i64` or `Ordering`. Never replace a ported `sort` with `slice::sort_by`: the result differs for inconsistent comparators.

## regexes

Pyright's regexes have no `u` flag. Where the `regex` crate spells a JS escape differently, splice the `js_regex` fragment into the pattern:

| JavaScript | `regex` crate |
|---|---|
| `\s`, `\S`, `\s` inside `[...]` | `js_regex::WHITESPACE`, `NOT_WHITESPACE`, `WHITESPACE_IN_CLASS` |
| `\w`, `\W`, `\w` inside `[...]` | `js_regex::WORD`, `NOT_WORD`, `WORD_IN_CLASS` |
| `\d`, `\D` | `js_regex::DIGIT`, `NOT_DIGIT` |
| `\b` | `js_regex::WORD_BOUNDARY` |
| `.` | `js_regex::DOT` |
| `/i` on ASCII letters | `(?i-u:letters)` |
| `\s` test on one character | `string::is_js_whitespace_char(c)` |

Lookahead and code-unit matching of astral characters have no fragment; `notes/jsrt.md` lists each call site and how to port it.

## flags

`jsrt::flag_ops!(X)` gives a flags newtype `pub struct X(pub u32)` the `|`, `|=`, `&`, `&=`, `!` operators and `any`, for TypeScript bit-flag enums.
