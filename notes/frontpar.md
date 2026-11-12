# frontpar notes

Read, tokenize and parse source files on worker threads ahead of the checker (`docs/research/speed-levers.md`, lever 3). Binding stays on the main thread.

## design

`crates/pyright/src/parse_ahead.rs` (`docs/porting.md`, "parse ahead"):
- The main thread requests files; a worker reads one with the calls `SourceFile.getFileContent` makes (`stat`, the size limit, a UTF-8 read) and parses it as `SourceFile.parse` would.
- `SourceFile.parse` takes the result at pyright's point, after `isParseRequired`. A job no worker started is withdrawn and the main thread parses the file; a running job is waited for. `Program._parsedFileCount` counts at the same place as before.
- A file the worker cannot read, or that the file system maps to no single real file (`ReadOnlyFileSystem::real_file_path`), parses on the main thread, which reports the failure as pyright does.
- What makes a worker's result equal to a main-thread parse:
  - atoms come from one process-wide table (`jsrt::atom`), with a per-thread table of seen atoms in front, so a repeated text takes no lock;
  - a file parse numbers its nodes from 1 and takes one block of ids when it ends (`with_node_id_block`), so a file's ids stay consecutive (`AnalyzerNodeInfoStore` keeps its dense range);
  - a worker allocates nothing a collector traces (`jsrt::heap::forbid_collected_allocation`): its output lives in its leaked arena and needs no root. The parser's or-pattern addendum runs under `jsrt::heap::allow_temporaries`, served off both heaps;
  - token region chunks recycle through one process-wide pool, so a region the main thread drops goes back to the workers without an `munmap`.
- Idle workers call `mi_collect(true)` after the main thread took results: mimalloc reclaims a block freed by another thread only when its owner allocates or collects.

What the main thread requests (`after_parse`, after `Program._parseFile` resolved the file's imports):
1. The next 16 user files of the check loop.
2. Every import of a parsed user file.
3. Wanted names: a user file wants every name, a module wants the names some wanted `from m import ...` imports from it. A module is requested once a file imports a wanted name from it; a name wanted from a module parsed already passes on at once.
4. Module reads: `m.x` in a parsed file wants `x` from the module `import m`, `import p.m as m` or `from p import m` binds, and requests that module. Outside the user files only reads outside function bodies count: the check evaluates such a body only to infer a return type.
5. Guessed imports: a worker that finishes a module the check wants names from requests what the module imports those names from, before the main thread resolves the module's imports. It looks a relative import up next to the module (a stub, a source file, then a package) and an absolute one in the main thread's earlier resolutions (`Table::modules`). Without this step the main thread waited for 1,047 files, 0.69 s; with it, for 178 files, 0.10 s.

The main thread and the workers share one table by path: the jobs, the files the main thread parsed, the wanted names and the learned module paths.

## measurements

Trace of a serial pydantic-ai run at 04ac81d: 6,696 files parsed (120.5 MB of source), 5,972 more registered as imports and never parsed (61.0 MB). Files first imported by a user file: 319, all parsed. Files first imported by another file: 10,303, 42% parsed.

Share of parsed source bytes requested before the main thread parses the file (simulated on the trace, then measured):

| requests | parsed bytes covered | never parsed |
|---|---|---|
| user files and their imports | 41.7% | none |
| plus wanted names | 49.8% | 14 files, 0.2 MB |
| plus module reads in user files | 54.4% (simulated) | 14 files, 0.2 MB |
| plus module reads in every parsed file | 87.2% (simulated) | 687 files, 10.1 MB |
| the same, outside function bodies outside user files | 85.8% (simulated); 5,385 of 6,696 files measured | 285 files, 4.2 MB |
| every import of every parsed file | 100% | 5,972 files, 61.0 MB |

Distance from request to take, in files taken, at the last row: median 20, 90th percentile 978, 99th 3,909. Evicting untaken results after a few hundred takes would drop many hits.

Resident set at the end of the check (`PYRIGHT_RS_GC_CENSUS=1`, pydantic-ai, cb3aeb6): 5,667 MiB without workers, 5,860 MiB with two, 5,904 MiB with four (0b1cbde).
- Workers that never parse (every job withdrawn): 5,684 MiB, so the mechanism itself costs about 17 MiB.
- Dropping the 320 untaken results just before the census frees 61 MiB; their trees, about 35 MiB, stay in the worker arenas.
- About 100 MiB more stays in the workers' malloc heaps. `MIMALLOC_PURGE_DELAY=0` does not release it, nor does capping the source held by untaken results (`PYRIGHT_RS_PARSE_AHEAD_WAITING_MIB`, tried at 8, 16 and 32 MiB, not landed: the peak moved by under 20 MiB and the main thread parsed 380 files itself instead of 67). The untaken results peak at 38.9 MB of source.

Two workers wait as little as four: 156 waits, 0.079 s, against 176 waits, 0.101 s.

## result

f13afd7 against main 13a4fa8, three interleaved runs each; `dist` from `bench/build-dist.sh`:

| corpus, build | instructions | peak RSS |
|---|---|---|
| pydantic-ai, release | 197.6 G → 200.3 G (+1.35%) | 5.45 → 5.67 GiB (+0.22) |
| pydantic-ai, release, switch off | 198.1 G (+0.23%) | 5.49 GiB (+0.04) |
| pydantic-ai, dist | 162.2 G → 164.4 G (+1.38%) | 5.48 → 5.69 GiB (+0.21) |
| samples, release | 7.11 G → 7.24 G (+1.8%) | 0.44 → 0.48 GiB |
| samples, dist | 5.71 G → 5.81 G (+1.75%) | 0.45 → 0.48 GiB |

Wall time: other lanes held the load average between 2 and 20, and the medians follow the load more than the branch. A main run against the branch run right after it, both under load 5: release −1.05, −0.65 and +0.07 s. The dist runs ran under load 15–20, too high for a pair; at d3c6308 under load 5–8 the dist median dropped 0.12 s. At 1229e5e (user files and their imports, four workers, load 3–4) the median dropped 1.08 s. samples moves less than its run-to-run spread.

CPU time: in the same pairs user plus system time rose 0.9 to 1.8 s. The workers spend 2.6 s of CPU on 5,650 parses (`PYRIGHT_RS_PARSE_AHEAD_LOG`, load 8.8), about twice what the main thread spends per file.

Where the gain goes: the main thread's front end (`--stats`: Read Source Files, Tokenize, Parse) drops from about 1.6 s to 0.3 s, Bind from 1.46 s to 1.27 s; the wall drops by 0.65 s, the median of the quiet pairs. Main-thread cycle samples under load 4–7 did not separate contention from the workers' SMT siblings from the load: 59.3k against 72.2k, 58.7k against 67.1k, 66.4k against 61.9k.

The remaining serial front end: 1,272 files no rule requests, which the main thread parses itself, and 0.1 s of waits.

The switch off costs 0.23% instructions and 20 to 45 MiB of peak against main. Not measured further; the changes that run with the switch off are the atom table, the region pool and the node id counter.

## Signature changes

- `jsrt::Atom` is `Send` and `Sync`; `Atom::lookup` finds text any thread interned.
- `parser::parse_nodes::_NEXT_NODE_ID` is a process-wide `AtomicU32`; `with_node_id_block(parse, nodes)`.
- `jsrt::heap::forbid_collected_allocation()`, `jsrt::heap::allow_temporaries(f)`.
- `ReadOnlyFileSystem::real_file_path(uri) -> Option<String>`, default `None`; implemented by `RealFileSystem`, `FileSystemMappingImpl`, `ReadOnlyAugmentedFileSystem` and `PyrightFileSystem`.
- `DiagnosticSink::peek()`, for check mode.
- `ParseOptions` derives `PartialEq`.
- `SourceFile::parse_ahead_request`; `SourceFile.parse`'s read and parse moved into `SourceFile::read_and_parse`, unchanged; `SourceFile::make_parse_options` out of `parse_file`.

## Shared-file edits

- `crates/jsrt/src/atom.rs`: process-wide sharded table.
- `crates/jsrt/src/region.rs`: process-wide chunk pool.
- `crates/jsrt/src/heap.rs`: the two functions above, a unit test.
- `crates/pyright/src/parser/parse_nodes.rs`: the node id counter and blocks.
- `crates/pyright/src/parser/parser.rs`: `ParseOptions: PartialEq`; the or-pattern addendum under `allow_temporaries`.
- `crates/pyright/src/analyzer/source_file.rs`, `program.rs`: the take and the requests.
- `crates/pyright/src/common/file_system.rs`, `real_file_system.rs`, `file_system_mapping.rs`, `readonly_augmented_file_system.rs`, `pyright_file_system.rs`: `real_file_path`.
- `crates/pyright/src/common/diagnostic_sink.rs`: `peek`.
- `docs/porting.md` ("layout", "strings", "parse ahead", "switches"), `docs/collector-design.md` ("roots"), `tools/miri.sh`.

## Needs from others

## Doubts

- A worker reads a file earlier than pyright would. A file that changes on disk during the check is read at another moment; pyright's CLI has the same exposure between its enumeration and its parse.
- In a `dist` build (`panic = "abort"`) a parser panic on a worker aborts the process, also for a requested file pyright never parses. Every parser panic is a port bug.
- `--stats`: "Parse" includes the main thread's wait for workers; "Read Source Files" and "Tokenize" count only files parsed on the main thread. Workers' time is in `PYRIGHT_RS_PARSE_AHEAD_LOG`.
- The census counts the worker arenas under "malloc heap and the rest", and `PYRIGHT_RS_ALLOC_STATS` counts only the main thread's allocations.
