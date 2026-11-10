#!/usr/bin/env bash
# Oracle corpus: pyright's test samples at the spec commit. Config: samples.json.
set -euo pipefail
source "$(dirname "$0")/setup.sh"
require_checkout "$HOME/pyright" "$spec"
plain_venv
run_corpus samples
