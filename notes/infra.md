# infra notes

## Signature changes

- `Uri` is one Copy handle (`common/uri/uri.rs`) for `FileUri`, `EmptyUri` and `ConstantUri`. `==` is `Uri.equals`; `Hash` uses the key, so `Uri` replaces the `uri.key` strings pyright uses as map keys. `Uri::ptr_eq` is `===`. `WebUri` is not ported.
- `Diagnostic` and `DiagnosticAddendum` are shared: `P<Diagnostic>`, `P<DiagnosticAddendum>`. `DiagnosticSink` methods take `&self`.
- `TextRangeDiagnosticSink` derefs to `DiagnosticSink` and holds `&'static TextRangeCollection<TextRange>`.
- `ParameterizedString::format(&[(key, value)])`: pass keys in the TypeScript object-literal order; format numbers with JS `String(n)` first.
- Localizer accessors: `LocMessage::import_resolve_failure()`, `LocAddendum::...`; names are the TypeScript names in snake_case.
- `DiagnosticRule` variants keep the TypeScript member names; `as_str` gives the string value.
- `Program`, `SourceFile` and `SourceFileInfo` are shared (`P<...>`) with `&self` methods: the import lookup re-enters the program while a file binds. No `RefCell` borrow is held across a call into another file.
- `SourceFile::bind(config, import_lookup, future_imports)` and `SourceFile::check(config)`: the binder-typed arguments (`builtinsScope`, `cellChainIndex`, `nodeInfoContext`) and the checker arguments come with the binder and checker.
- `AnalyzerService::run_timers` stands in for Node's event loop: `scheduleReanalysis` arms a flag instead of a `setTimeout`, and the CLI fires it after `enumerateSourceFiles(0)`, the order Node runs them in.
- Import resolution: `ImportResolver` methods take `&ExecutionEnvironment` and `&ImportedModuleDescriptor`, return `P<ImportResult>` (shared; `filterImplicitImports` makes a new one), and take every TypeScript default argument explicitly (`get_module_name_for_import(uri, env, allow_invalid_module_name, detect_py_typed)`, `get_import_roots(env, for_logging)`, `get_python_search_paths(logger)`). Caches are cells, so every method takes `&self`.
- `ParentDirectoryCache::new()` takes no `_importRootGetter`: it would capture the resolver that owns the cache. `check_valid_path(fs, source, root, import_root_getter)` takes it instead and calls it as lazily as pyright does.
- `ImportResolverFileSystem`, `TypeshedInfoProvider`, `FileSystemMapping` and `FileSystemMappingState` are type aliases of their one implementation. The `ServiceKeys.importResolverFileSystem` / `typeshedInfoProvider` overrides (test-only in pyright) are not ported.
- `ImportResolverFileSystemImpl::readdir_entries_sync` returns `&'static [Dirent]` and never fails, as in pyright; its `ReadOnlyFileSystem` impl wraps that and makes the methods outside pyright's `Pick<FileSystem, ...>` unreachable. `get_files_in_directory` returns `&'static [Uri]`.
- Memoized typeshed results are `P` handles: `TypeshedThirdPartyPackageMapResult = P<(JsMap<String, Vec<Uri>>, Vec<Uri>)>`, `get_std_lib_module_version_info -> P<JsMap<String, SupportedVersionInfo>>`.
- `SupportPartialStubs` is a trait (`PartialStubService`, `NoOpPartialStubs`). `process_partial_stub_packages` drops `allowMoving`: no caller passes it.
- `ServiceProvider::new(fs, console, detector)`; `partial_stubs()` creates the `PartialStubService` on first use (`serviceProviderExtensions.ts`). Set `sp.partial_stubs` first to register another.

## Shared-file edits

- `crates/pyright/src/common/file_system.rs`: `FileSystem::map_directory`, `Disposable` (vscode-jsonrpc), `MapDirectoryFilter`, `Dirent::virtual_dirent` (`VirtualDirent`).
- `crates/pyright/src/common/real_file_system.rs`: `map_directory` (a no-op, as in pyright).
- `crates/pyright/src/common/service_provider.rs`: `partial_stubs` field and method, `ServiceProvider::new`.
- `crates/pyright/src/lib.rs` declares `file_system_mapping`, `partial_stub_service`, `pyright_file_system`, `readonly_augmented_file_system` and the `#[cfg(test)] tests` harness; `crates/pyright/src/analyzer/mod.rs` declares `import_statement_utils` and `typeshed_info_provider`.
- new `crates/pyright/src/file_system_mapping.rs` ports `fileSystemMapping.ts`, which `readonlyAugmentedFileSystem.ts` needs.
- new `crates/pyright/src/tests/harness/` holds the test doubles: `vfs/filesystem.rs` (in-memory `TestFileSystem`) and `test_access_host.rs` (`TestAccessHost`).
- `crates/jsrt/src/node_path.rs` (new): Node `path.posix` (normalize, resolve, basename, extname, dirname), fuzzed against node 26.
- `crates/jsrt/src/vscode_uri.rs` (new): npm `vscode-uri` 3.1.0 `URI.file`, `URI.parse`, `toString`, `fsPath`, `Utils.resolvePath`.
- `crates/jsrt/src/jsonc.rs` (new): npm `jsonc-parser` 3.3.1 `parse` with `allowTrailingComma`, diffed against it on 3000 inputs (`crates/jsrt/tests/data/jsonc.jsonl`).
- `crates/jsrt/src/smol_toml.rs` (new): npm `smol-toml` 1.7.0 `parse` on the `toml` crate (TOML 1.1.0), values as `serde_json::Value`.
- `crates/jsrt/src/command_line_args.rs` (new): npm `command-line-args` 5.2.1 in strict mode, with its error kinds.
- `crates/jsrt/Cargo.toml`: `serde_json` (preserve_order), `toml` 1.2.1 (preserve_order), `regex`.
- `crates/jsrt/src/lib.rs`: declares the modules above.
- `crates/pyright/src/oracle/diag.rs` and `oracle/dumper.rs` (new): the diag stage and the fork's dump hooks (env, file writes, `oracle-run.txt`). `oracle_dump_parse` writes the tokens stage through frontend's `dump_tokenizer_output`; tree, bind, flow and types write nothing until their dumpers exist.
- `crates/pyright/src/oracle/mod.rs`: declares `dumper`.
- `crates/pyright/src/analyzer/mod.rs`: declares `analysis`, `background_analysis_program`, `circular_dependency`, `service_utils`.
- Switched to `jsrt::number` (main 8a4b41e): deleted `crates/jsrt/src/infra_shims.rs`; `python_version.rs` parses with `jsrt::number::parse_int`.
- Switched to `jsrt::string::is_js_whitespace_char` (jsrt agent): deleted the copy in `common/core.rs`.

## Needs from others

- binder: `analyzer_file_info.rs` on branch `binder` is this branch's file plus `AnalyzerFileInfo.builtins_scope` and `ImportLookupResult.symbol_table`; take that version at merge. `source_file.rs` on `binder` only adds an `IPythonMode` shim; take this branch's. After the binder lands, infra wires `run_binder` in `source_file.rs` and the stand-ins in `program.rs` (`bind_file` builtins scope, `look_up_import_impl` result, `drop_parse_and_bind_info`) and `SourceFile::mark_reanalysis_required` (`getDunderAllInfo`).
- binder: `symbolNameUtils` (`isDunderName`, `isPrivateOrProtectedName`).

## Doubts

- Zip and egg archives on an import path are not read: pyright treats a `.zip`/`.egg` file with zip magic as a directory.
- `getModulePath()` is `PYRIGHT_RS_MODULE_PATH` or the executable's directory; `typeshed-fallback` must sit there (or one level up). How typeshed ships is open.
- Config `_convertBoolean`/`_convertDiagnosticLevel` print through Node's global `console.log`, i.e. to stdout even under `--outputjson`; ported as stdout.
- Only English messages ship. pyright picks a translation only when the locale is a known key (`de`, `de-DE`, `VSCODE_NLS_CONFIG`); `LANG=de_DE.UTF-8` stays English. `load_strings_for_locale` panics for a non-English pick.
- POSIX host only: `path.sep` is `/`, no drive-letter or backslash handling beyond what pyright does on Linux.
- `createFileUri`'s cache: pyright keeps an 8192-entry LRU (`common/uri/memoization.ts` `cacheStaticFunc`); the port interns every Uri, so two Uris with one key are always the same object. pyright compares Uris by identity in `CircularDependency.isEqual`, `typeshedPath === projectRoot` (`_ensureDefaultOptions`), `execEnv.root !== typeshedRoot` (`_resolveBestAbsoluteImport`), the search-path dedup `Set`, and `resolvedUris[0] !== this._uri` (`sourceFile.ts` `_resolveImports`); the port compares keys there. They differ only when pyright created the two objects more than 8192 `createFileUri` calls apart: then pyright treats a file importing itself as an import.
- `Diagnostic.message` is a Rust `String`, so a message that quotes source text with a lone surrogate loses it. The JSON report and the diag dump would print U+FFFD where pyright prints `\udxxx`.
- Not ported, with no effect on a CLI run: the realpath alias index in `program.ts` (it bridges invalidation between symlink twins when files change), the cache manager and `emptyCache` (reference runs never empty the cache), the log tracker, file watchers, edit mode, open files, notebook cell chains and shadow files.
- `enumerateSourceFiles` pushes auto-excluded directories onto the `ConfigOptions` the service was constructed with, which nothing reads afterwards; the port keeps only the `setConfigOptions` call that follows.
- `--threads N` runs single-threaded; `--watch`, `--verifytypes` and `--createstub` stop with exit 2. `--stats` prints no timing details.
- A TOML parse error prints `JSON.stringify(e)` of smol-toml's `TomlError` in pyright; the port prints the error message. Stderr only.
- `importFailureInfo` is the logger's own array in pyright (`ImportLogger.getLogs()` returns it), so a result shows lines logged after it was built. `_resolve_best_absolute_import` copies the logger's final lines into the result it returns; no other path logs after building a kept result.
- `typeshedInfoProvider.ts` logs `JSON.stringify(e)` when VERSIONS cannot be read; for a Node error that is `{"errno":...,"code":...,"syscall":...,"path":...}`. `FsError` keeps only a message, so the port logs that message. Only `--verbose` output with an unreadable VERSIONS file differs.
- `filterImplicitImports` copies a result with `Object.assign`, sharing its maps; the port deep-clones `ImportResult`, so `import x` of a package with many submodules copies its implicit-import map. Output is the same.
