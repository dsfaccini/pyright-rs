#!/usr/bin/env bash
# Build pinned pyright and record baseline runs of a full pydantic-ai check.
# One command per line: set -e does not stop on a failure inside an && chain.
set -euxo pipefail
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.bun/bin:$PATH"
corepack enable --install-directory "$HOME/.local/bin"
cd ~/pyright
COREPACK_ENABLE_DOWNLOAD_PROMPT=0 pnpm install --frozen-lockfile
pnpm --dir packages/pyright run build
cd ~/pydantic-ai
UV_PYTHON=3.13 UV_PYTHON_PREFERENCE=only-managed uv sync --frozen --all-extras --no-extra mcp-tasks --all-packages --group lint
PYRIGHT=~/pyright/packages/pyright/index.js
node "$PYRIGHT" --version
/usr/bin/time -v node "$PYRIGHT" > ~/baseline/default-heap.txt 2> ~/baseline/default-heap.time || true
/usr/bin/time -v node --max-old-space-size=16384 "$PYRIGHT" --verbose > ~/baseline/big-heap-verbose.txt 2> ~/baseline/big-heap-verbose.time || true
/usr/bin/time -v node --max-old-space-size=16384 "$PYRIGHT" --outputjson > ~/baseline/reference.json 2> ~/baseline/reference.time || true
touch ~/baseline/DONE
