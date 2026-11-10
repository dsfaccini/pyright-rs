# jsrt

What pyright gets from JavaScript and V8: object references, ordered collections, and JS string, number and sort semantics. Call these where `docs/porting.md` says JavaScript and Rust differ.

Every helper is tested against Node 26 through fixtures in `fixtures/`. Regenerate them with `node crates/jsrt/fixtures/gen.mjs` (Node 26 on PATH), then run `cargo test -p jsrt`.

## objects and collections

| JavaScript | Rust |
|---|---|
| object reference, `===` on objects | `P<T>` (`P::new`, `==` is identity) |
| `Map`, `Set` | `JsMap`, `JsSet` (delete with `shift_remove`) |
| lookup-only map or set | `FxHashMap`, `FxHashSet` (never iterate) |

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
