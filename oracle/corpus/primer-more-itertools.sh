#!/usr/bin/env bash
# Oracle corpus: mypy_primer project more-itertools at its pin. Config: primer-more-itertools.json.
set -euo pipefail
source "$(dirname "$0")/setup.sh"
primer_setup primer-more-itertools
run_corpus primer-more-itertools
