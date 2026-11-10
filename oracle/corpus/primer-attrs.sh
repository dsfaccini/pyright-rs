#!/usr/bin/env bash
# Oracle corpus: mypy_primer project attrs at its pin. Config: primer-attrs.json.
set -euo pipefail
source "$(dirname "$0")/setup.sh"
primer_setup primer-attrs
run_corpus primer-attrs
