# pyright-rs

A Rust port of the command-line check of [pyright](https://github.com/microsoft/pyright) 1.1.414.
`pyright-rs` takes pyright's command line and config, and prints the same output, JSON and text, with the same exit code.

- `oracle/corpus/check.sh` compares pyright-rs with pyright on each recorded corpus in `oracle/corpus/`: the `--outputjson` diagnostics and every internal stage (tokens, tree, bind, flow, types). The tool is described in `oracle/README.md`.
- Timings against pyright: `bench/step6.md`.
- Not ported: the language server, `--watch`, `--verifytypes` and `--createstub`. `--threads` is accepted; every run is single-threaded.

## install

Release tarballs hold a Linux x86_64 binary (glibc 2.39 or later, x86-64-v2) with pyright 1.1.414's `typeshed-fallback` beside it.
Release tags are the pyright version followed by a build number.

```sh
tag=v1.1.414-1
name=pyright-rs-$tag-x86_64-unknown-linux-gnu
curl -fsSLO https://github.com/dsfaccini/pyright-rs/releases/download/$tag/$name.tar.gz
curl -fsSLO https://github.com/dsfaccini/pyright-rs/releases/download/$tag/$name.tar.gz.sha256
sha256sum -c $name.tar.gz.sha256
tar -xzf $name.tar.gz
$name/pyright-rs --outputjson
```

`pyright-rs` reads `typeshed-fallback` from its own directory, or from `$PYRIGHT_RS_MODULE_PATH/typeshed-fallback` when that variable is set.
pyright's exit codes are 0 to 4. A panic in the release binary aborts the process with SIGABRT, which a shell reports as exit code 134.

## build

`cargo build --release` builds `target/release/pyright-rs`.
`bench/build-dist.sh` builds the release binary, `target/dist/pyright-rs`, with PGO and BOLT; `tools/release.sh` packages it.

## license

MIT, as pyright: `LICENSE`. Release tarballs add `THIRD-PARTY-LICENSES.txt` and typeshed's `LICENSE`.
