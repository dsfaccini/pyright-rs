#!/usr/bin/env python3
"""Run a checker on one oracle corpus and record its dumps and JSON output.

Usage: run.py CONFIG.json

The config (oracle/corpus/<name>.json) holds the corpus name, the working
directory, the checker arguments, and optionally a pyrightconfig.json to write
into the working directory. `~/` and `{typeshed}` in strings are expanded.

Environment:
  ORACLE_CHECKER   checker command line (default: the pyright fork under node
                   with a 16 GB heap). Set it to run pyright-rs.
  ORACLE_OUT       output directory (default ~/pyright-rs-data/reference/<corpus>).
                   It must not exist yet.
  ORACLE_GATES     1 runs the reference gates (default: 1 when ORACLE_CHECKER is unset).
  ORACLE_TYPESHED  the checker's bundled typeshed directory, used by {typeshed}
  PYRIGHT_RS_MODULE_PATH  where pyright-rs looks for typeshed-fallback (default: the
                   fork's packages/pyright/dist, the reference runs' typeshed)
                   (default: the fork's dist/typeshed-fallback).

Output in ORACLE_OUT:
  command.json   the resolved command, working directory and environment
  dump/          PYRIGHT_ORACLE_DUMP output (format: oracle/FORMAT.md)
  output.json    --outputjson stdout
  stderr.txt, time.txt
  gates.txt      reference gate results (only with ORACLE_GATES=1)
"""

import json
import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
COMPARE = HERE.parent / "compare.py"
FORK = Path.home() / "pyright-oracle"
DEFAULT_CHECKER = f"node --max-old-space-size=16384 {FORK}/packages/pyright/index.js"
DEFAULT_TYPESHED = FORK / "packages/pyright/dist/typeshed-fallback"


def expand(value, typeshed):
    if isinstance(value, str):
        value = value.replace("{typeshed}", str(typeshed))
        return os.path.expanduser(value) if value.startswith("~/") else value
    if isinstance(value, list):
        return [expand(v, typeshed) for v in value]
    if isinstance(value, dict):
        return {k: expand(v, typeshed) for k, v in value.items()}
    return value


def run_checker(checker, args, cwd, out_dir, stem, dump_dir):
    env = dict(os.environ)
    env.pop("PYRIGHT_ORACLE_DUMP", None)
    env.pop("PYRIGHT_ORACLE_STAGES", None)
    if dump_dir is not None:
        env["PYRIGHT_ORACLE_DUMP"] = str(dump_dir)
    # pyright-rs finds typeshed through PYRIGHT_RS_MODULE_PATH. Point it at the fork's
    # bundled copy, so the stdlib resolves to the same files as the reference run.
    env.setdefault("PYRIGHT_RS_MODULE_PATH", str(FORK / "packages/pyright/dist"))
    argv = ["/usr/bin/time", "-v", "-o", str(out_dir / f"{stem}.time.txt"), *checker, *args]
    with open(out_dir / f"{stem}.stdout", "wb") as stdout, open(out_dir / f"{stem}.stderr.txt", "wb") as stderr:
        proc = subprocess.run(argv, cwd=cwd, env=env, stdout=stdout, stderr=stderr)
    # pyright exits 0 (no errors) or 1 (errors reported); anything else is a failed run.
    if proc.returncode not in (0, 1):
        sys.exit(f"run.py: {stem} run exited with {proc.returncode}; see {out_dir}/{stem}.stderr.txt")
    return proc.returncode


def cache_empty_count(dump_dir):
    run_file = dump_dir / "oracle-run.txt"
    if not run_file.is_file():
        sys.exit(f"run.py: {run_file} missing; the dumper did not finish")
    for line in run_file.read_text().splitlines():
        if line.startswith("cacheEmptyCount: "):
            return int(line.split(": ", 1)[1])
    sys.exit(f"run.py: no cacheEmptyCount in {run_file}")


def comparable_json(path):
    report = json.loads(Path(path).read_text())
    report.pop("time", None)
    report.get("summary", {}).pop("timeInSec", None)
    return report


def main(argv):
    if len(argv) != 1:
        sys.exit(__doc__)
    config_path = Path(argv[0]).resolve()
    typeshed = Path(os.environ.get("ORACLE_TYPESHED", str(DEFAULT_TYPESHED)))
    config = expand(json.loads(config_path.read_text()), typeshed)
    corpus = config["corpus"]

    checker_env = os.environ.get("ORACLE_CHECKER")
    checker = shlex.split(os.path.expanduser(checker_env or DEFAULT_CHECKER))
    gates = os.environ.get("ORACLE_GATES", "0" if checker_env else "1") == "1"
    out_dir = Path(os.environ.get("ORACLE_OUT", str(Path.home() / "pyright-rs-data/reference" / corpus)))
    if out_dir.exists():
        sys.exit(f"run.py: {out_dir} exists; move or delete it first")

    cwd = Path(config["cwd"])
    cwd.mkdir(parents=True, exist_ok=True)
    if "pyrightconfig" in config:
        (cwd / "pyrightconfig.json").write_text(json.dumps(config["pyrightconfig"], indent=4) + "\n")

    args = config["args"]
    out_dir.mkdir(parents=True)
    command = {
        "corpus": corpus,
        "config": str(config_path),
        "cwd": str(cwd),
        "checker": checker,
        "args": ["--outputjson", *args],
        "env": {"PYRIGHT_ORACLE_DUMP": str(out_dir / "dump")},
    }
    (out_dir / "command.json").write_text(json.dumps(command, indent=4) + "\n")

    # Reference run: dumps on, --outputjson.
    run_checker(checker, ["--outputjson", *args], cwd, out_dir, "run", out_dir / "dump")
    (out_dir / "run.stdout").rename(out_dir / "output.json")
    (out_dir / "run.stderr.txt").rename(out_dir / "stderr.txt")
    (out_dir / "run.time.txt").rename(out_dir / "time.txt")
    empties = cache_empty_count(out_dir / "dump")
    results = [f"cacheEmptyCount: {empties} ({'ok' if empties == 0 else 'FAIL'})"]
    failed = empties != 0

    if gates:
        # Gate: --verbose prints no "Emptying type cache", and a second run gives identical dumps.
        verbose_dump = out_dir / "gate-verbose-dump"
        run_checker(checker, ["--verbose", *args], cwd, out_dir, "gate-verbose", verbose_dump)
        verbose_log = (out_dir / "gate-verbose.stdout").read_bytes() + (out_dir / "gate-verbose.stderr.txt").read_bytes()
        emptied = b"Emptying type cache" in verbose_log
        results.append(f"verbose run prints no 'Emptying type cache': {'FAIL' if emptied else 'ok'}")
        compare = subprocess.run(
            [sys.executable, str(COMPARE), "--max-reports", "5", str(out_dir / "dump"), str(verbose_dump)],
            stdout=subprocess.PIPE,
            text=True,
        )
        (out_dir / "gate-determinism.txt").write_text(compare.stdout)
        same_dumps = compare.returncode == 0
        results.append(f"two runs give byte-identical dumps: {'ok' if same_dumps else 'FAIL'}")
        if same_dumps:
            shutil.rmtree(verbose_dump)
        failed = failed or emptied or not same_dumps

        # Gate: --outputjson output is identical with dumps off and on.
        run_checker(checker, ["--outputjson", *args], cwd, out_dir, "gate-nodump", None)
        same_json = comparable_json(out_dir / "output.json") == comparable_json(out_dir / "gate-nodump.stdout")
        results.append(f"--outputjson identical with dumps off and on: {'ok' if same_json else 'FAIL'}")
        failed = failed or not same_json

    (out_dir / "gates.txt").write_text("\n".join(results) + "\n")
    summary = comparable_json(out_dir / "output.json").get("summary", {})
    print(f"{corpus}: {summary}")
    print("\n".join(results))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
