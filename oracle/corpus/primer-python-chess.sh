#!/usr/bin/env bash
# Oracle corpus: mypy_primer project python-chess at its pin. Config: primer-python-chess.json.
set -euo pipefail
source "$(dirname "$0")/setup.sh"
primer_setup primer-python-chess
run_corpus primer-python-chess
