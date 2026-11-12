#!/usr/bin/env bash
# Package target/dist/pyright-rs (bench/build-dist.sh) as a release tarball:
# the binary, the spec's typeshed-fallback beside it, LICENSE and
# THIRD-PARTY-LICENSES.txt.
#
# Usage: tools/release.sh TAG OUTDIR
#   TAG is the pyright version followed by a build number, e.g. v1.1.414-1.
# Environment:
#   TYPESHED  typeshed-fallback to ship (default: the spec checkout's)
# Output: OUTDIR/pyright-rs-TAG-<host>.tar.gz and OUTDIR/pyright-rs-TAG-<host>.tar.gz.sha256
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
tag=$1 out=$2
name=pyright-rs-$tag-$(rustc -vV | sed -n 's/^host: //p')
typeshed=${TYPESHED:-$HOME/pyright/packages/pyright-internal/typeshed-fallback}
stage=$out/$name
rm -rf "$stage"
mkdir -p "$stage"
cp "$ROOT/target/dist/pyright-rs" "$ROOT/LICENSE" "$stage/"
cp -r "$typeshed" "$stage/typeshed-fallback"
python3 -I "$ROOT/tools/third_party_licenses.py" > "$stage/THIRD-PARTY-LICENSES.txt"
tar -C "$out" -czf "$out/$name.tar.gz" "$name"
(cd "$out" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
rm -rf "$stage"
echo "$out/$name.tar.gz"
