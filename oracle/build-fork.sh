#!/usr/bin/env bash
# Recreate and build the pyright oracle fork: a git worktree of ~/pyright on branch
# `oracle`, at the spec commit plus oracle/pyright-oracle.patch.
# Builds it the way bench/baseline.sh builds ~/pyright. Never modifies ~/pyright's checkout.
#
# Environment: PYRIGHT (spec clone, default ~/pyright), PYRIGHT_ORACLE_FORK (default ~/pyright-oracle).
set -euxo pipefail
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.bun/bin:$PATH"

spec=a28cce4077d87b80bead12de809f79845e39c8ba
pyright="${PYRIGHT:-$HOME/pyright}"
fork="${PYRIGHT_ORACLE_FORK:-$HOME/pyright-oracle}"
patch="$(cd "$(dirname "$0")" && pwd)/pyright-oracle.patch"

if [ ! -e "$fork" ]; then
    if git -C "$pyright" rev-parse --verify --quiet refs/heads/oracle > /dev/null; then
        git -C "$pyright" worktree add "$fork" oracle
    else
        git -C "$pyright" worktree add -b oracle "$fork" "$spec"
        git -C "$fork" apply --index "$patch"
        git -C "$fork" commit -q -m "Add the pyright-rs oracle dumper"
    fi
fi

# The fork must be exactly the spec plus the patch, committed.
test -z "$(git -C "$fork" status --porcelain)"
git -C "$fork" diff "$spec" HEAD | cmp - "$patch"

corepack enable --install-directory "$HOME/.local/bin"
cd "$fork"
COREPACK_ENABLE_DOWNLOAD_PROMPT=0 pnpm install --frozen-lockfile
pnpm --dir packages/pyright run build
node packages/pyright/index.js --version
