# Milestone 1 checkpoint: tokenizer, parser, binder

Measured 2026-11-10 on devbox (16 cores, 62 GB), at `main` after the milestone 1 merges.

## correctness

Command, per corpus: `ORACLE_CHECKER=target/release/pyright-rs python3 oracle/corpus/run.py oracle/corpus/<corpus>.json`, then
`oracle/compare.py --diag-lists parse,comment,bind,taskList --ignore-reference-only` on the tokens, tree, bind, flow and diag stages.

| corpus | files pyright-rs parses and binds | differing | candidate-only | reference-only (ignored) |
|---|---|---|---|---|
| samples | 1,353 | 0 | 0 | 76 |
| typeshed | 740 | 0 | 0 | 47 |
| primer-attrs | 34 | 0 | 0 | 32 |
| primer-bidict | 12 | 0 | 0 | 18 |
| primer-more-itertools | 7 | 0 | 0 | 31 |
| primer-packaging | 23 | 0 | 0 | 52 |
| primer-python-chess | 9 | 0 | 0 | 69 |
| pydantic-ai | 2,047 | 0 | 0 | 4,649 |

- Reference-only files are files pyright parses because its checker follows imports. pyright-rs has no checker yet.
- Those files are covered by the stage harnesses, which run the Rust stages on every file of a reference dump:
  `examples/oracle_dump.rs` (tokens, tree) and `examples/oracle_bind.rs` (bind, flow, bind diagnostics). Both match on all 8 corpora, 6,696 files on pydantic-ai.
  The bind harness replays each file's import results from the reference, so for these files import resolution is checked only through the CLI run above.
- `filesAnalyzed` in the JSON summary equals the reference on every corpus.

## parse and bind time

Same 6,696 files: every file pyright parses on pydantic-ai.

| phase | pyright 1.1.414 (`--stats`) | pyright-rs (`oracle_bind --timing`) | ratio |
|---|---|---|---|
| Tokenize | 2.32 s | 1.22 s | 1.9x |
| Parse | 5.51 s | 1.84 s | 3.0x |
| Bind | 9.16 s | 2.70 s | 3.4x |
| total | 16.99 s | 5.76 s | 2.9x |

- pyright: `node --max-old-space-size=16384 ~/pyright/packages/pyright/index.js --stats` in `~/pydantic-ai`, one run. The run took 2:24 wall and 9.3 GB peak. Output: `~/pyright-rs-data/bench/ts-stats-pydantic-ai.txt`.
- pyright-rs: release build, mean of three runs (spread under 5%). Phase timers wrap the same calls as pyright's `TimingStat`s; file reading and import replay are outside them.
- Not like for like:
  - pyright's timers run inside a full check, so its phases pay for garbage collection of checker objects.
  - pyright's Bind timer also covers files parsed on demand while binding (`TimingStat` reentrancy).
  - pyright-rs has no checker running alongside.

`pyright-rs --stats` on pydantic-ai, end to end without dumps (2,047 files, mean of three runs): Tokenize 0.50 s, Parse 0.76 s, Bind 0.70 s, Resolve Imports 0.26 s; 2.7 s wall, 2.6 GB peak RSS.

## memory

- The arena holds 4.87 GB after parsing and binding the 6,696 files, which are 120.5 MB of source: about 40 bytes per source byte. Heap-owned `Vec`s and strings come on top.
- Node sizes: `ParseNode` 160 bytes (every node pays for the largest `ParseNodeDetails` variant), `Token` 72, `Declaration` 184, `FlowNode` 80, `Symbol` 88.
- pyright's whole check peaks at 9.3 GB. Step 5 adds types to the arena, so this is the first size to watch. Remedies, unmeasured: box the large detail variants, 32-bit handles (`PLAN.md` step 2), interning.
