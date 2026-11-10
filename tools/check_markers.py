"""Checks the `// <path>.ts:<line>` markers in the Rust port.

Each marker must point at a TypeScript line that declares the item that follows
the marker: the Rust name must appear in the TypeScript line, both lowercased
and reduced to letters and digits (so `fn lazy_import_python_3_15` matches
`test('LazyImport - Python 3.15'`).

    uv run tools/check_markers.py [rust files...]   (default: every file under crates/pyright/src)

Exits 1 and lists the mismatches when any marker is wrong.
"""

import os
import re
import sys
from pathlib import Path

PYRIGHT_SRC = Path(os.environ.get("PYRIGHT_ROOT", Path.home() / "pyright")) / "packages/pyright-internal/src"
REPO = Path(__file__).resolve().parent.parent
MARKER = re.compile(r"^\s*//\s*([\w/]+\.ts):(\d+)\s*$")
ITEM = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+fn|fn|struct|enum|trait|type|const|static|mod|impl)\s+([A-Za-z_][A-Za-z0-9_]*)"
)
FIELD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:")
# Rust names whose TypeScript declaration uses another word.
ALIASES = {"new": ["constructor"]}


def normalize(text: str) -> str:
    return re.sub(r"[^0-9a-z]", "", text.lower())


def candidates(rust_name: str) -> list[str]:
    name = normalize(rust_name)
    names = [name]
    if name.endswith("details"):
        names.append(name[: -len("details")])
    names.extend(ALIASES.get(rust_name, []))
    return names


def check_file(path: Path) -> list[str]:
    errors = []
    lines = path.read_text().splitlines()
    for i, line in enumerate(lines):
        m = MARKER.match(line)
        if not m:
            continue
        ts_path, ts_line = m.group(1), int(m.group(2))
        ts_file = PYRIGHT_SRC / ts_path
        if not ts_file.exists():
            errors.append(f"{path}:{i + 1}: no file {ts_path}")
            continue
        ts_lines = ts_file.read_text().splitlines()
        if ts_line < 1 or ts_line > len(ts_lines):
            errors.append(f"{path}:{i + 1}: {ts_path} has no line {ts_line}")
            continue
        item = None
        for following in lines[i + 1 : i + 6]:
            if following.strip().startswith(("#[", "//", "///")) or not following.strip():
                continue
            im = ITEM.match(following) or FIELD.match(following)
            if im:
                item = im.group(1)
            # A thread_local! block or macro: use the first identifier after `static`.
            elif "static" in following:
                sm = re.search(r"static\s+([A-Za-z_][A-Za-z0-9_]*)", following)
                item = sm.group(1) if sm else None
            break
        if item is None:
            errors.append(f"{path}:{i + 1}: no item after the marker")
            continue
        target = normalize(ts_lines[ts_line - 1])
        if not any(c in target for c in candidates(item)):
            errors.append(f"{path}:{i + 1}: `{item}` not declared at {ts_path}:{ts_line}: {ts_lines[ts_line - 1].strip()}")
    return errors


def main() -> int:
    files = [Path(a) for a in sys.argv[1:]] or sorted((REPO / "crates/pyright/src").rglob("*.rs"))
    errors = [e for f in files for e in check_file(f)]
    for e in errors:
        print(e)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
