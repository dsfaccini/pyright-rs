#!/usr/bin/env python3
"""Compare two oracle dump directories (format: oracle/FORMAT.md).

Reports, per file and stage, the first differing line with context, and the
files present on only one side. Exits 0 when the dumps match, 1 when they
differ, 2 on a usage error.
"""

import argparse
import sys
from pathlib import Path

STAGES = ("tokens", "tree", "bind", "flow", "types", "diag")


def parse_args(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path, help="reference dump directory (pyright fork)")
    parser.add_argument("candidate", type=Path, help="candidate dump directory (pyright-rs)")
    parser.add_argument(
        "--stage",
        action="append",
        choices=STAGES,
        help="compare only this stage; repeatable (default: all)",
    )
    parser.add_argument(
        "--path",
        action="append",
        default=[],
        metavar="PREFIX",
        help="compare only files whose mapped path starts with PREFIX, e.g. typeshed/stdlib/; repeatable",
    )
    parser.add_argument("--context", type=int, default=3, help="context lines before a difference (default 3)")
    parser.add_argument(
        "--max-reports",
        type=int,
        default=20,
        help="print details for at most this many differing files (default 20; 0 for none)",
    )
    parser.add_argument("--list", action="store_true", help="list every differing and missing file")
    return parser.parse_args(argv)


def collect(root, stages, prefixes):
    """Map of 'stage/mapped path' (without .txt) to file path."""
    files = {}
    for stage in stages:
        stage_dir = root / stage
        if not stage_dir.is_dir():
            continue
        for path in stage_dir.rglob("*.txt"):
            if not path.is_file():
                continue
            mapped = path.relative_to(stage_dir).as_posix()[: -len(".txt")]
            if prefixes and not any(mapped.startswith(p) for p in prefixes):
                continue
            files[f"{stage}/{mapped}"] = path
    return files


def indent_of(line):
    return len(line) - len(line.lstrip(b" "))


def ancestors(lines, index):
    """Lines that enclose lines[index] by indentation, outermost first."""
    result = []
    if index >= len(lines):
        return result
    level = indent_of(lines[index])
    for i in range(index - 1, -1, -1):
        if level == 0:
            break
        line_level = indent_of(lines[i])
        if line_level < level:
            result.append((i, lines[i]))
            level = line_level
    result.reverse()
    return result


def show(line):
    return line.decode("utf-8", errors="replace")


def split_lines(data):
    lines = data.split(b"\n")
    if lines and lines[-1] == b"":
        lines.pop()
    return lines


def first_difference(ref_lines, cand_lines):
    for i, (a, b) in enumerate(zip(ref_lines, cand_lines)):
        if a != b:
            return i
    return min(len(ref_lines), len(cand_lines))


def report(key, ref_data, cand_data, context, out):
    ref_lines = split_lines(ref_data)
    cand_lines = split_lines(cand_data)
    index = first_difference(ref_lines, cand_lines)
    out.write(f"DIFF {key} line {index + 1} (reference {len(ref_lines)} lines, candidate {len(cand_lines)} lines)\n")

    start = max(0, index - context)
    enclosing = [(i, line) for i, line in ancestors(ref_lines, index) if i < start]
    for i, line in enclosing:
        out.write(f"  {i + 1:>7} | {show(line)}\n")
    if enclosing and enclosing[-1][0] < start - 1:
        out.write("          | ...\n")
    for i in range(start, index):
        out.write(f"  {i + 1:>7} | {show(ref_lines[i])}\n")
    ref_line = show(ref_lines[index]) if index < len(ref_lines) else "<end of file>"
    cand_line = show(cand_lines[index]) if index < len(cand_lines) else "<end of file>"
    out.write(f"- {index + 1:>7} | {ref_line}\n")
    out.write(f"+ {index + 1:>7} | {cand_line}\n")
    out.write("\n")


def main(argv):
    args = parse_args(argv)
    for root in (args.reference, args.candidate):
        if not root.is_dir():
            print(f"compare: not a directory: {root}", file=sys.stderr)
            return 2

    stages = args.stage or list(STAGES)
    ref_files = collect(args.reference, stages, args.path)
    cand_files = collect(args.candidate, stages, args.path)

    counts = {stage: {"same": 0, "differ": 0, "only_ref": 0, "only_cand": 0} for stage in stages}
    differing = []
    only_ref = sorted(set(ref_files) - set(cand_files))
    only_cand = sorted(set(cand_files) - set(ref_files))
    out = sys.stdout

    reports = 0
    for key in sorted(set(ref_files) & set(cand_files)):
        stage = key.split("/", 1)[0]
        ref_data = ref_files[key].read_bytes()
        cand_data = cand_files[key].read_bytes()
        if ref_data == cand_data:
            counts[stage]["same"] += 1
            continue
        counts[stage]["differ"] += 1
        differing.append(key)
        if reports < args.max_reports:
            report(key, ref_data, cand_data, args.context, out)
            reports += 1

    for key in only_ref:
        counts[key.split("/", 1)[0]]["only_ref"] += 1
    for key in only_cand:
        counts[key.split("/", 1)[0]]["only_cand"] += 1

    if args.list:
        for key in differing:
            out.write(f"differs: {key}\n")
    shown = only_ref if args.list else only_ref[:10]
    for key in shown:
        out.write(f"missing in candidate: {key}\n")
    if len(shown) < len(only_ref):
        out.write(f"... {len(only_ref) - len(shown)} more missing in candidate (--list shows all)\n")
    shown = only_cand if args.list else only_cand[:10]
    for key in shown:
        out.write(f"missing in reference: {key}\n")
    if len(shown) < len(only_cand):
        out.write(f"... {len(only_cand) - len(shown)} more missing in reference (--list shows all)\n")

    out.write(f"{'stage':<8} {'same':>8} {'differ':>8} {'ref only':>9} {'cand only':>10}\n")
    totals = {"same": 0, "differ": 0, "only_ref": 0, "only_cand": 0}
    for stage in stages:
        c = counts[stage]
        for k in totals:
            totals[k] += c[k]
        out.write(f"{stage:<8} {c['same']:>8} {c['differ']:>8} {c['only_ref']:>9} {c['only_cand']:>10}\n")
    out.write(
        f"{'total':<8} {totals['same']:>8} {totals['differ']:>8} {totals['only_ref']:>9} {totals['only_cand']:>10}\n"
    )

    failed = totals["differ"] + totals["only_ref"] + totals["only_cand"]
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
