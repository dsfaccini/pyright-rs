#!/usr/bin/env bash
# Oracle corpus: pydantic-ai at its pin, venv synced like CI's lint job. Config: pydantic-ai.json.
set -euo pipefail
source "$(dirname "$0")/setup.sh"
cfg="$corpus_dir/pydantic-ai.json"
require_checkout "$HOME/pydantic-ai" "$(json_field "$cfg" pin.commit)"
(cd "$HOME/pydantic-ai" && eval "$(json_field "$cfg" pin.sync)")
run_corpus pydantic-ai
