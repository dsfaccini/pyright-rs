# oracle

The comparison tool from step 1 of `PLAN.md`. A pyright fork dumps its internal state per file and stage; pyright-rs writes the same dumps; `compare.py` reports where they first differ.

| file | what |
|---|---|
| `FORMAT.md` | the dump format, the whole contract for the Rust dumper |
| `pyright-oracle.patch` | the fork: pyright 1.1.414 (`a28cce407`) plus the dumper |
| `build-fork.sh` | recreates and builds the fork at `~/pyright-oracle` from `~/pyright` and the patch |
| `compare.py` | compares a reference and a candidate dump directory |
| `corpus/` | one recorded config (`<corpus>.json`) and one script (`<corpus>.sh`) per corpus, and the shared runner `run.py` |

## the fork

`~/pyright-oracle` is a git worktree of `~/pyright` on branch `oracle`, one commit on top of the spec commit.
`build-fork.sh` creates it when missing, checks that it equals the spec plus `pyright-oracle.patch`, then installs and builds it like `bench/baseline.sh` builds `~/pyright`.

The patch adds `src/oracle/oracleDumper.ts` and five hooks:

- `SourceFile.parse`: tokens and tree stages, after the parser returns;
- `SourceFile.bind`: bind and flow stages, after the binder publishes its node info;
- `Program.emptyCache`: counts cache empties;
- the CLI completion callback in `pyright.ts`: types and diag stages, once, before the report;
- `TypeEvaluator.oracleReadTypeCacheEntry` and `SourceFile.getOracleDiagnosticLists`: read-only accessors for those stages.

With `PYRIGHT_ORACLE_DUMP` unset, every hook returns at once and the fork behaves like upstream.

```
PYRIGHT_ORACLE_DUMP=/tmp/dump PYRIGHT_ORACLE_STAGES=tokens,tree \
    node ~/pyright-oracle/packages/pyright/index.js --outputjson
```

## reference dumps

Each corpus script runs the fork with a 16 GB heap and writes to `~/pyright-rs-data/reference/<corpus>/`:
`command.json` (the exact command), `dump/` (the dumps), `output.json` (`--outputjson`), `stderr.txt`, `time.txt`, `gates.txt`.
The output directory must not exist yet.

```
oracle/corpus/samples.sh
```

| corpus | what | files checked | files dumped | dump size | wall time |
|---|---|---|---|---|---|
| samples | pyright's 1,353 test sample files | 1,350 | 1,429 | 159 MB | 0:11 |
| typeshed | the bundled typeshed stdlib stubs | 737 | 787 | 183 MB | 0:09 |
| pydantic-ai | pydantic-ai `ead5e549b`, CI lint venv | 2,046 | 6,696 | 4.4 GB | 12:37 |
| primer-attrs | attrs, `src/attrs src/attr typing_tests` | 33 | 66 | 39 MB | 0:03 |
| primer-bidict | bidict | 11 | 30 | 20 MB | 0:01 |
| primer-more-itertools | more-itertools | 6 | 38 | 31 MB | 0:03 |
| primer-packaging | packaging, `src` | 22 | 75 | 53 MB | 0:03 |
| primer-python-chess | python-chess | 8 | 78 | 67 MB | 0:04 |

Pins are in each `<corpus>.json`. The samples and typeshed corpora use an empty uv-managed Python 3.13 venv (`~/pyright-rs-data/venvs/plain`) for search paths, the primer projects one empty 3.13 venv each. pydantic-ai uses its own config and `.venv`, synced as in `bench/baseline.md`.

## comparing pyright-rs

Run the same corpus script with the Rust checker, then compare:

```
ORACLE_CHECKER="$HOME/pyright-rs/target/release/pyright-rs" ORACLE_OUT=/tmp/rs-samples oracle/corpus/samples.sh
oracle/compare.py ~/pyright-rs-data/reference/samples/dump /tmp/rs-samples/dump --stage tokens --stage tree
```

- The candidate's `output.json` is compared to the reference's `output.json` without the `time` and `summary.timeInSec` fields.
- For the typeshed corpus, set `ORACLE_TYPESHED` to the Rust side's bundled typeshed directory.
- `compare.py` prints, for each differing file, the first differing line, the lines that enclose it by indentation, and a few lines of context; then the files present on one side only, and counts per stage. It exits 0 when everything matches, 1 otherwise, 2 on a usage error.

`compare.py` options:

| option | effect |
|---|---|
| `--stage S` | compare only stage `S`; repeatable |
| `--path PREFIX` | compare only files whose mapped path starts with `PREFIX`, for example `typeshed/stdlib/`; repeatable |
| `--diag-lists NAMES` | in diag files, compare only the named lists (comma-separated, from `parse`, `comment`, `bind`, `checker`, `taskList`, `final`); each list is its header line and the lines under it. Reported line numbers stay those of the full files. |
| `--ignore-reference-only` | files present only in the reference do not fail the comparison and are not listed (`--list` lists them); the summary still counts them under `ref only` |
| `--list` | list every differing and missing file |
| `--max-reports N` | print details for at most `N` differing files (default 20) |
| `--context N` | context lines before a difference (default 3) |

Milestone 1 (tokenizer, parser, binder) has no checker yet, so its check compares the parse-and-bind stages and the diag lists those phases fill. Pyright parses more files than a parse-and-bind run does, because the checker pulls in imports:

```
oracle/compare.py ~/pyright-rs-data/reference/samples/dump /tmp/rs-samples/dump \
    --stage tokens --stage tree --stage bind --stage flow --stage diag \
    --diag-lists parse,comment,bind,taskList --ignore-reference-only
```

## gates

`run.py` runs the gates for every reference run (`ORACLE_GATES=1`, the default with the fork as checker):

- cacheEmptyCount: the dump's `oracle-run.txt` records no `Program.emptyCache` call.
- verbose: a second run with `--verbose` (dumps on) prints no "Emptying type cache". `--outputjson` and `--verbose` cannot be combined, hence the extra run.
- determinism: that second run's dumps are byte-identical to the reference dumps.
- dumps off and on: a third run with dumps off gives the same `--outputjson` output, without the time fields.

Results, 2026-11-10 on devbox:

| corpus | cacheEmptyCount | verbose | determinism | dumps off and on |
|---|---|---|---|---|
| samples | ok | ok | ok | ok |
| typeshed | ok | ok | ok | ok |
| pydantic-ai | ok | pending | pending | pending |
| primer-attrs | ok | ok | ok | ok |
| primer-bidict | ok | ok | ok | ok |
| primer-more-itertools | ok | ok | ok | ok |
| primer-packaging | ok | ok | ok | ok |
| primer-python-chess | ok | ok | ok | ok |

The pydantic-ai reference `--outputjson` is also identical to `bench/pydantic-ai-reference.json`, made by upstream pyright at the same pins, without the time fields.
Every corpus's dumps are also byte-identical between two builds of the fork (before and after the printed-type memo).

## format decisions

- Global counters never print. Parse nodes print as `<Type>@<start>+<length>`, flow nodes are renumbered per file in first-visit order, scopes per file in walk order, and a flow assignment's symbol id prints as its scope and name.
- File paths under `typeshed-fallback/` map to `typeshed/…`, so a checker with its typeshed elsewhere produces the same dumps.
- The types stage runs once at the end of the run and reads only the evaluator's main type cache. Printing never runs inference: a return type not yet inferred prints as `Unknown`.
- `importFailureInfo` never prints: the import resolver fills it only with `--verbose`.
