# bind notes

Single-thread speed of the front end: tokenizer, parser, binder, import resolution, and the file reads and text conversions around them. Output and every dump stay identical.

## Signature changes

- `parser::parse_tree_utils::get_child_nodes<L: ChildNodeList>(node, get_string_annotation) -> L`: generic over the list it fills. `ParseNodeArray` (a `Vec`) and `ChildNodes` (eight children inline, then a `Vec`) implement `ChildNodeList`. Callers that want a `Vec` name it: `get_child_nodes::<ParseNodeArray>(...)`. `ParseTreeWalker::visit_node` returns `ChildNodes`.
- `parser::tokenizer::detach_substring` returns the atom and whether the text is ASCII.
- `IdentifierToken::create_with_ascii(start, length, value, ascii, comments)`: `create` for a value the caller knows to be ASCII or not; an ASCII value skips the NFKC scan.
- `uri::memoization::cache_static_func(key: &str, ...)`: the key is borrowed and copied only on a miss.
- `jsrt::string::string_from_utf16_lossy(units)`: `String::from_utf16_lossy` with ASCII narrowed in bulk. `jsrt::string::Utf8Segments`: a `str` as runs of ASCII bytes and single other chars.
- `Binder::_bind_name_value_to_scope` takes the name as an `Atom`.
- `path_utils::reduce_path_components<S: AsRef<str> + Clone>(&[S]) -> Vec<S>`: generic over the component type, so `get_path_components` reduces slices.
- `ImportResult.implicit_imports` and `filtered_implicit_imports` are `Option<Rc<JsMap<String, ImplicitImport>>>`: shared between a result and its copies, as `Object.assign` shares them.

## Shared-file edits

- `crates/jsrt/src/string.rs`: `JsString::from(&str)` widens the ASCII runs of non-ASCII text in bulk; a `str`'s UTF-16 length sums its ASCII runs and other chars; `string_from_utf16_lossy`, `Utf8Segments`; a test.
- `crates/pyright/src/common/file_system.rs`: `read_file_text_sync` keeps valid UTF-8 without a copy.
- `crates/pyright/src/common/string_utils.rs`: `get_string_fingerprint` hashes ASCII runs four bytes a step; a test against the one-unit loop.
- `crates/pyright/src/common/uri/file_uri.rs`, `memoization.rs`: `createFileUri`'s cache key in a reused buffer.
- `crates/pyright/src/analyzer/import_resolver.rs`: `PYRIGHT_RS_USER_FILE_MEMO`.
- `crates/pyright/src/analyzer/parse_tree_walker.rs`, `analyzer/parse_tree_utils.rs`, `oracle/{bind,tree,types}.rs`, `parser/parser_tests.rs`: the generic `get_child_nodes`.
- `crates/pyright/src/common/path_utils.rs`, `tests/path_utils.rs`: the generic `reduce_path_components`.
- `crates/pyright/src/analyzer/import_result.rs`, `oracle/bind.rs`, `examples/oracle_bind.rs`, `parse_ahead.rs` (in the merge with `main` 3a9a365): the shared implicit import maps.
- `docs/porting.md` ("switches"): `PYRIGHT_RS_USER_FILE_MEMO`, `PYRIGHT_RS_PARSER_CURRENT_TOKEN`.

## Needs from others

## Doubts

- `_maxChildDepthMap` became a slot per node id (`DepthByNodeId` in `parser.rs`) without a switch: every read is `get(id) ?? 0` and nothing iterates or deletes, so it is the same map in another representation, as the dense `AnalyzerNodeInfoStore` is.
- The identifier cache (`tokenizer.ts:1397`) keeps its slots and hash, but stores ASCII atoms only, so a non-ASCII identifier always misses. Atoms of equal text are equal, so which path interns a name changes no result.

## Where the front end spends

pydantic-ai reads, tokenizes, parses and binds 6,696 files, 120.5 MB of source, 15.1 M tokens. Half the bytes (60.1 MB) are in the 1,436 files that hold any non-ASCII byte, though they hold only 259 K such bytes: a whole-file `is_ascii` test sent those files down per-char paths in the reads, the fingerprint and the UTF-16 conversion.

IBS op samples of a frame-pointer build, pydantic-ai, attributed to the leaf-most phase frame (`Tokenizer::tokenize`, `Parser::parse_source_file`'s parse loop, `SourceFile::parse`'s read and import closures, `SourceFile::bind`'s closure); thousands of samples of 1 M ops:

| phase | 04ac81d | 3c93412 | e8af323 |
|---|---|---|---|
| tokenize | 7.39 | 6.48 | 5.82 |
| parse | 11.51 | 7.69 | 6.36 |
| bind | 8.78 | 7.84 | 7.52 |
| resolve imports | 3.27 | 2.72 | 2.25 |
| read | 2.63 | 1.36 | 1.35 |
| parse, outside the timers | 1.56 | 0.77 | 0.76 |
| front end | 35.1 | 26.9 | 24.1 |
| whole run | 218.7 | 209.4 | 205.6 |

At e8af323 Read still held 0.40 of `js_len` (the 4-byte-lead count ran byte by byte), which 81a19f8 removes.

IBS also tags user instructions that fault on fresh memory, so allocation sites (`alloc_node`, `place_token`, `Symbol::new`, `P::new` of node info) collect kernel time; read their samples with that in mind.

## Phase times

`main` 3920e3e against this branch merged with it (72b3fa7), `release`, devbox loaded by other agents; the main session re-measures on a quiet machine.

`pyright-rs --stats` on pydantic-ai, two paired runs each under one lock hold at a time, load average 7 to 11:

| phase | 3920e3e | 72b3fa7 |
|---|---|---|
| Read Source Files | 0.27, 0.32 s | 0.21, 0.25 s |
| Tokenize | 0.79, 0.84 s | 0.69, 0.76 s |
| Parse | 0.93, 0.98 s | 0.67, 0.73 s |
| Resolve Imports | 0.66, 0.71 s | 0.60, 0.64 s |
| Bind | 1.79, 1.93 s | 1.44, 1.82 s |

The branch's start (`main` 04ac81d) against the second batch's tip (81a19f8, which also holds the other lanes' work merged into `main` since), the same way, load average 7 to 10:

| phase | 04ac81d | 81a19f8 |
|---|---|---|
| Read Source Files | 0.33, 0.33 s | 0.22, 0.20 s |
| Tokenize | 0.99, 0.92 s | 0.77, 0.70 s |
| Parse | 1.17, 1.12 s | 0.73, 0.68 s |
| Resolve Imports | 0.74, 0.85 s | 0.54, 0.51 s |
| Bind | 2.06, 2.41 s | 1.64, 1.44 s |
| the five together | 5.29, 5.63 s | 3.90, 3.53 s |

`examples/oracle_bind.rs --timing` on the 6,696 files of the reference dump, median of three interleaved runs, load average about 11: Tokenize 0.94 to 0.86 s, Parse 1.24 to 1.14 s, Bind 2.04 to 2.02 s. Bind's ops fell 11% (IBS table above) but its time barely moved: much of it waits on memory, and its allocation sites fault on fresh program-heap pages.

`bench/measure.sh`, median of 3:

| corpus | 3920e3e | 72b3fa7 |
|---|---|---|
| pydantic-ai | 212.80 G instructions, 5.53 GiB peak RSS | 201.49 G (−5.3%), 5.53 GiB |
| samples | 7.57 G, 0.44 GiB | 7.23 G (−4.5%), 0.44 GiB |

## Attempts

pydantic-ai, `release`, `bench/measure.sh`; one run unless noted. Under load (load average up to 12 while other agents ran) a single run can sit 0.5% high: marking threads spin, and `instructions:u` counts every thread. The kept changes are the branch's commits, in order.

| change | pydantic-ai | samples |
|---|---|---|
| `main` 04ac81d | 212.98 G | 7.55 G |
| parser: token-stream helpers `#[inline]` (kept, de8406c) | 212.27 G | 7.53 G |
| the same with `#[inline(always)]` on peek and next token only | 212.27 G | 7.53 G |
| `read_file_text_sync` keeps valid UTF-8 (kept, 550d6a7) | 211.48 G | 7.51 G |
| a `str`'s UTF-16 length by `chars().count()` (kept, b4c4de0) | 211.44 G | 7.52 G |
| `JsString::from` widens ASCII runs (kept, 641b7e5) | 210.62 G | 7.51 G |
| fingerprint over ASCII runs, four bytes a step (kept, 5f0fa12) | 209.89 G | 7.51 G |
| identifiers intern from a stack buffer (kept, afac6e5) | 209.27 G | 7.44 G |
| `string_from_utf16_lossy` (kept, e8c4ad3) | 208.61 G | 7.42 G |
| peek as one clamped index, token checks `#[inline(always)]` (kept, 6cdc7e5) | 205.89 G | 7.34 G |
| walker children inline (kept, f778222) | 205.20 G | 7.32 G |
| binder lookups by atom (kept, e41f278) | 204.81 G | 7.30 G |
| `PYRIGHT_RS_USER_FILE_MEMO` (kept, 3e66256) | 204.07 G | 7.28 G |
| `createFileUri` key buffer (kept, 3c93412) | 203.88 G | 7.29 G |
| ASCII identifiers skip the NFKC scan (kept, 40708d6) | 203.52 G | 7.28 G |
| string literals skip ordinary runs (kept, 6e35162) | 202.98 G | 7.28 G |
| indentation and comment runs (kept, c978b25) | 202.81 G (3 runs) | 7.28 G |
| `PYRIGHT_RS_PARSER_CURRENT_TOKEN` (kept, 9b5e814; 3 runs) | 201.79 G | 7.25 G |
| `_maxChildDepthMap` slots (kept, 91641fc; 3 runs) | 201.69 G | 7.21 G |

Second batch, on `main` 00fc773 (which holds the first batch), median of 3:

| change | pydantic-ai | samples |
|---|---|---|
| `main` 00fc773 | 200.41 G, 5.53 GiB; at `PYRIGHT_RS_GC_MIN_MIB=64` 205.63 G, 5.05 GiB | 7.21 G, 0.45 GiB |
| `getPathComponents` reduces slices (kept, e9a0245) | −0.12 G (measured on the first batch's tip) | |
| shared implicit import maps (kept, 68ccf18) | −0.28 G, peak 5.52 to 5.37 GiB (on the first batch's tip) | |
| unescaping copies plain runs (kept, 7554118) | −0.50 G (on the first batch's tip) | |
| `getArgsByRuntimeOrder` in one array (kept, 7bb69a7) | −0.19 G on the first batch's tip; 199.35 G after the four on 00fc773 | 7.19 G |
| docstring helpers borrow their lists (kept, e8af323) | 199.25 G | 7.19 G |
| a `str`'s UTF-16 length over ASCII runs (kept, 81a19f8) | 198.86 G, 5.38 GiB; at 64 MiB 204.07 G, 4.90 GiB | 7.19 G, 0.44 GiB |

The peak falls by the same 0.15 GiB at the default trigger and at 64 MiB, so the live set shrank: each filtered `ImportResult`, a program object that is never freed, copied both implicit import maps.

The batch merged with `main` 3a9a365 (frontpar's parse workers), against that `main`; median of 3, the 64 MiB runs once:

| corpus | `main` 3a9a365 | merge 8eafe82 |
|---|---|---|
| pydantic-ai | 200.34 G, 5.69 GiB; at 64 MiB 205.66 G, 5.19 GiB | 198.76 G (−0.8%), 5.56 GiB; at 64 MiB 204.16 G, 5.03 GiB |
| samples | 7.22 G, 0.48 GiB | 7.21 G, 0.48 GiB |

With parse ahead on, `--stats` Read Source Files and Tokenize count only the files the main thread parses, and Parse includes its waits for the workers, so the phase tables above stop at 81a19f8.

Measured and dropped:

- `_peek_token` bounds as two comparisons instead of `clamp`: 18.474 G to 18.465 G in the parse harness (below), no change.
- The current token's switch, parse harness: no switch 17.10 G; the mode tested on every peek 17.72 G; the mode tested only where the index is written (kept) 17.43 G; from 18.47 G before.
- The identifier cache at 8,192 slots instead of 2,048: tokenize harness 7.457 G to 7.476 G, worse (each file clears more slots, and the table misses the CPU caches more).
- `PYRIGHT_RS_ALLOC_STATS` read through an atomic byte instead of a `OnceLock` in `record` (every `P::new` and `P::new_eval`): samples 7.24 G to 7.23 G, within the spread. Skipping the check entirely gave 7.19 G, so the check costs about 0.7% of the run, most of it in the checker; a cheaper form would need the switch read before the first allocation.

## What is left

Front-end IBS samples at e8af323 (thousands of 1 M ops), the candidates above 0.12 (0.5% of the front end), and why each stays:

- `get_child_nodes::<ChildNodes>` 0.96 and `Binder::walk` 0.43 in Bind: a call per walked node with a match over every node type; the per-node heap array is gone. Walking children lazily would read a node's children after walking the earlier ones, where pyright reads them all first; not tried.
- The tokenizer's main loop 2.54: per-char work spread over `_try_identifier` (half), whitespace, character dispatch and token creation, each 1 to 6% of it.
- `_parse_arithmetic_factor` 0.43, `_parse_atom` 0.37, `_parse_arg_list` 0.25: the expression grammar's levels, each with its own token checks, as in the TypeScript.
- `get_string_fingerprint` 0.74: the FNV half is one multiply per UTF-16 unit, 120 M units; the polynomial half already folds four units per step.
- Node info `get`, `get_or_create`, `cold_mut` 0.29, 0.27, 0.22, `Symbol::new` 0.27, `alloc_node`, `place_token`: dense already, or allocation sites whose samples include faults on fresh program-heap pages (kernel time IBS tags on the faulting store).
- `convert_offset_to_position` 0.28: declaration ranges; it already tries the last hit and its neighbor before the binary search.
- Code-flow expression sets (`IndexMap<Atom, ()>::insert_full` and rehash) 0.29: pyright's `Set<string>` per execution scope; atoms already make each insert a pointer hash.
- `get_file_level_directives` 0.29: pyright's walk over every token's comments.
- The scratch region's drop 0.12: it visits every scratch token to drop the few whose details own memory; keeping a list of those would change unsafe code.
- Resolve Imports: `create_file_uri` 0.18 and readdir per-entry `combine_paths`, pyright's `cacheStaticFunc` LRU and per-entry Uris; samples without a symbol 0.34 (libc and the vDSO).

## How to reproduce

- Phase profile: build with `CARGO_TARGET_DIR=target/fp RUSTFLAGS="-C force-frame-pointers=yes" cargo build --release`, record `perf record -e ibs_op/cnt_ctl=1,swfilt=1/u -c 1000000 --call-graph fp` on the pydantic-ai command under the shared lock, `perf script --no-inline -F ip,sym`, and attribute each sample to its leaf-most phase frame (the frames in the table above).
- Front-end harness: a scratch example that reads the 6,696 files the CLI opens (from `strace -e openat`), converts each to UTF-16, then tokenizes or parses it, under `perf stat -e instructions:u`. Totals, each with the 1.01 G that reading and converting take: tokenize 8.32 G at 3c93412, 7.46 G at c978b25; parse, which tokenizes too, 18.47 G at c978b25, 17.30 G at 91641fc. The harness passes no content hash, so the parser fingerprints each file itself. A tokenizer or parser A/B takes seconds instead of a pydantic-ai run.
- Line attribution inside a fat-LTO function: per-IP IBS counts from `perf script -F ip,sym,symoff`, mapped through `objdump -d -l --inlines` to the inline chain.
