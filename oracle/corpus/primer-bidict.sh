#!/usr/bin/env bash
# Oracle corpus: mypy_primer project bidict at its pin. Config: primer-bidict.json.
set -euo pipefail
source "$(dirname "$0")/setup.sh"
primer_setup primer-bidict
run_corpus primer-bidict
