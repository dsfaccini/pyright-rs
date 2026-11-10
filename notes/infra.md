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
- `crates/jsrt/src/lib.rs`: declares the two modules above.
- Switched to `jsrt::number` (main 8a4b41e): deleted `crates/jsrt/src/infra_shims.rs` (`number_to_string` had no callers left); `python_version.rs` parses with `jsrt::number::parse_int`.

## Needs from others

- frontend: `Tokenizer.isPythonIdentifier`, the parser entry point and `ParseFileResults`.
- binder: `symbolNameUtils` (`isDunderName`, `isPrivateOrProtectedName`) and `Binder.bindModule`.

## Doubts

- Zip and egg archives on an import path are not read: pyright treats a `.zip`/`.egg` file with zip magic as a directory.
- `getModulePath()` is `PYRIGHT_RS_MODULE_PATH` or the executable's directory; `typeshed-fallback` must sit there (or one level up). How typeshed ships is open.
- Config `_convertBoolean`/`_convertDiagnosticLevel` print through Node's global `console.log`, i.e. to stdout even under `--outputjson`; ported as stdout.

- Only English messages ship. pyright picks a translation only when the locale is a known key (`de`, `de-DE`, `VSCODE_NLS_CONFIG`); `LANG=de_DE.UTF-8` stays English. `load_strings_for_locale` panics for a non-English pick.
- POSIX host only: `path.sep` is `/`, no drive-letter or backslash handling beyond what pyright does on Linux.
- `createFileUri`'s cache: pyright keeps an 8192-entry LRU; the port interns every Uri. No caller compares Uris by identity, so output is unchanged.
