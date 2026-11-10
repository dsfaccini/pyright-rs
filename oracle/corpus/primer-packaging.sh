#!/usr/bin/env bash
# Oracle corpus: mypy_primer project packaging at its pin. Config: primer-packaging.json.
set -euo pipefail
source "$(dirname "$0")/setup.sh"
primer_setup primer-packaging
run_corpus primer-packaging
