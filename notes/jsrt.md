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
- `serde_json` (dev): reads the fixtures.

## Unicode data

- Case mapping uses Rust std's `char::to_lowercase`/`to_uppercase`, which are Unicode 17 in Rust 1.99, plus ICU's final-sigma rule. `src/string/case_tables.rs` (Cased, Case_Ignorable) is generated from Node's regex property escapes by `fixtures/gen.mjs`.
- When the Rust toolchain or Node moves to a new Unicode version, regenerate the fixtures and rerun `cargo test -p jsrt`: the per-code-point tests show any drift.
