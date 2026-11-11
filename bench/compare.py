#!/usr/bin/env python3
"""Time pyright against pyright-rs binaries on recorded corpora, dumps off.

Usage: bench/compare.py BIN[,BIN...] CORPUS...   (RUNS=3 by default)
Prints one markdown row per corpus: median wall time and CPU time, and the maximum
peak RSS in GiB, for pyright and for each binary, then pyright's wall time over each
binary's. pyright is the corpus's recorded
checker (`command.json`) without PYRIGHT_ORACLE_DUMP, which matches upstream.
pydantic-ai runs of pyright-rs hold /tmp/pyright-rs-pydantic-ai.lock and a 30G
memory cap, as bench/measure.sh does.
"""
import json, os, pathlib, statistics, subprocess, sys

RUNS = int(os.environ.get("RUNS", "3"))
REF = pathlib.Path.home() / "pyright-rs-data/reference"
ENV = dict(os.environ, PYRIGHT_RS_MODULE_PATH=str(pathlib.Path.home() / "pyright-oracle/packages/pyright/dist"))
ENV.pop("PYRIGHT_ORACLE_DUMP", None)


def run(cmd, cwd, wrap):
    walls, cpus, rss = [], [], []
    for _ in range(RUNS):
        r = subprocess.run(wrap + ["/usr/bin/time", "-f", "%e %U %S %M %x"] + cmd, cwd=cwd, env=ENV,
                           stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
        w, u, s, m, x = r.stderr.strip().splitlines()[-1].split()
        if x not in ("0", "1"):
            sys.exit(f"{cmd[0]} exited {x} in {cwd}")
        walls.append(float(w)); cpus.append(float(u) + float(s)); rss.append(int(m) / 1048576)
    return statistics.median(walls), statistics.median(cpus), max(rss)


def cell(r):
    return f"{r[0]:.2f} s, {r[1]:.2f} s CPU, {r[2]:.2f} GiB"


bins = [str(pathlib.Path(b).resolve()) for b in sys.argv[1].split(",")]
print("| corpus | pyright | " + " | ".join(pathlib.Path(b).parent.name for b in bins) + " | wall speedup |")
print("|---|---|" + "---|" * len(bins) + "---|")
for corpus in sys.argv[2:]:
    cmd = json.load(open(REF / corpus / "command.json"))
    rs_wrap = []
    if corpus == "pydantic-ai":
        rs_wrap = ["flock", "/tmp/pyright-rs-pydantic-ai.lock", "systemd-run", "--user", "--scope", "-q",
                   "-p", "MemoryMax=30G", "-p", "MemorySwapMax=0"]
    py = run(cmd["checker"] + cmd["args"], cmd["cwd"], [])
    rs = [run([b] + cmd["args"], cmd["cwd"], rs_wrap) for b in bins]
    ratios = ", ".join(f"{py[0] / r[0]:.1f}x" for r in rs)
    print(f"| {corpus} | {cell(py)} | " + " | ".join(cell(r) for r in rs) + f" | {ratios} |", flush=True)
