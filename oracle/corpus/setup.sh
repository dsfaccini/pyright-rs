# Shared setup for the oracle corpus scripts. Source it from a corpus script; do not run it.
export PATH="$HOME/.local/bin:$PATH"
corpus_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
spec=a28cce4077d87b80bead12de809f79845e39c8ba

# json_field <file> <dotted.key>
json_field() {
    python3 -c 'import json, sys
value = json.load(open(sys.argv[1]))
for key in sys.argv[2].split("."):
    value = value[key]
print(value)' "$1" "$2"
}

# require_checkout <dir> <commit>: the git checkout is at <commit> with no local changes.
require_checkout() {
    if [ "$(git -C "$1" rev-parse HEAD)" != "$2" ]; then
        echo "$1 is not at $2" >&2
        exit 1
    fi
    if [ -n "$(git -C "$1" status --porcelain)" ]; then
        echo "$1 has local changes" >&2
        exit 1
    fi
}

# The empty Python 3.13 venv that the samples and typeshed corpora use for search paths.
plain_venv() {
    local venv="$HOME/pyright-rs-data/venvs/plain"
    if [ ! -x "$venv/bin/python" ]; then
        UV_PYTHON_PREFERENCE=only-managed uv venv -q --python 3.13 "$venv"
    fi
}

# primer_setup <corpus>: clone the pinned mypy_primer project and create its empty venv.
primer_setup() {
    local cfg="$corpus_dir/$1.json"
    local name="${1#primer-}"
    local dir="$HOME/pyright-rs-data/corpus/primer/$name"
    local commit
    commit="$(json_field "$cfg" pin.commit)"
    if [ ! -d "$dir" ]; then
        git clone -q "$(json_field "$cfg" pin.repo)" "$dir"
    fi
    if ! git -C "$dir" cat-file -e "$commit^{commit}" 2> /dev/null; then
        git -C "$dir" fetch -q origin
    fi
    git -C "$dir" checkout -q --detach "$commit"
    require_checkout "$dir" "$commit"
    if [ ! -x "$dir-venv/bin/python" ]; then
        UV_PYTHON_PREFERENCE=only-managed uv venv -q --python "$(json_field "$cfg" pin.python)" "$dir-venv"
    fi
}

# run_corpus <corpus>: run the checker on the corpus (see run.py for the environment variables).
run_corpus() {
    exec python3 "$corpus_dir/run.py" "$corpus_dir/$1.json"
}
