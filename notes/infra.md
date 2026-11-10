# infra notes

## Signature changes

- `Uri` is one Copy handle (`common/uri/uri.rs`) for `FileUri`, `EmptyUri` and `ConstantUri`. `==` is `Uri.equals`; `Hash` uses the key, so `Uri` replaces the `uri.key` strings pyright uses as map keys. `WebUri` is not ported.
- `Diagnostic` and `DiagnosticAddendum` are shared: `P<Diagnostic>`, `P<DiagnosticAddendum>`. `DiagnosticSink` methods take `&self`.
- `TextRangeDiagnosticSink` derefs to `DiagnosticSink` and holds `&'static TextRangeCollection<TextRange>`.
- `ParameterizedString::format(&[(key, value)])`: pass keys in the TypeScript object-literal order; format numbers with JS `String(n)` first.
- Localizer accessors: `LocMessage::import_resolve_failure()`, `LocAddendum::...`; names are the TypeScript names in snake_case.
- `DiagnosticRule` variants keep the TypeScript member names; `as_str` gives the string value.

## Shared-file edits

- `crates/jsrt/src/node_path.rs` (new): Node `path.posix` (normalize, resolve, basename, extname, dirname), fuzzed against node 26.
- `crates/jsrt/src/vscode_uri.rs` (new): npm `vscode-uri` 3.1.0 `URI.file`, `URI.parse`, `toString`, `fsPath`, `Utils.resolvePath`.
- `crates/jsrt/src/infra_shims.rs` (new): JS `String(n)` for numbers. Move to jsrt's number module when it lands.
- `crates/jsrt/src/lib.rs`: declares the three modules above.
- `crates/pyright/src/common/text_range.rs`, `text_range_collection.rs`, `position_utils.rs`: SHIMS with the frontend branch's signatures. Take the frontend version wholesale on merge.

## Needs from others

- jsrt: JS `String(n)` for numbers (`infra_shims::number_to_string` until then).
- frontend: `TextRange`, `Position`, `Range`, `TextRangeCollection<TextRange>`, `convert_offsets_to_range`, `is_empty_range(&Range)` with the signatures the shims use.

## Doubts

- Only English messages ship. pyright picks a translation only when the locale is a known key (`de`, `de-DE`, `VSCODE_NLS_CONFIG`); `LANG=de_DE.UTF-8` stays English. `load_strings_for_locale` panics for a non-English pick.
- POSIX host only: `path.sep` is `/`, no drive-letter or backslash handling beyond what pyright does on Linux.
- `createFileUri`'s cache: pyright keeps an 8192-entry LRU; the port interns every Uri. No caller compares Uris by identity, so output is unchanged.
