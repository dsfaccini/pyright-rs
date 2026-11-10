#!/usr/bin/env bash
# Oracle corpus: the bundled typeshed stdlib stubs. Config: typeshed.json.
# ORACLE_TYPESHED names the checker's bundled typeshed directory (see run.py).
set -euo pipefail
source "$(dirname "$0")/setup.sh"
plain_venv
run_corpus typeshed
