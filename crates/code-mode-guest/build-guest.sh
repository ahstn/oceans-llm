#!/usr/bin/env bash
# Reproducible build of the Code Mode QuickJS guest artifact.
#
# Invoked by `mise run code-mode-guest-build` and `code-mode-guest-check`.
# Byte-identical output requires canonical inputs:
# - exact pinned Rust toolchain (host `stable` drifts over time),
# - `--remap-path-prefix` for every machine-dependent path that rustc bakes
#   into panic `Location` strings,
# - `-DNDEBUG -ffile-prefix-map` for the vendored quickjs-ng C sources so
#   `assert()` strings do not embed the build directory,
# - a throwaway target dir so stale incremental state never leaks in.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <output-wasm-path>" >&2
  exit 1
fi

out="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
toolchain="1.93.1"
crate_dir="$(cd "$(dirname "$0")" && pwd)"
target_dir="$(mktemp -d)"
trap 'rm -rf "$target_dir"' EXIT

rustup toolchain install "$toolchain" --profile minimal --target wasm32-wasip1

sysroot="$(rustup run "$toolchain" rustc --print sysroot)"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
export RUSTFLAGS="--remap-path-prefix=$cargo_home=/cargo-home --remap-path-prefix=$sysroot=/rust-sysroot --remap-path-prefix=$crate_dir=/code-mode-guest --remap-path-prefix=$target_dir=/cmg-target"
export CFLAGS_wasm32_wasip1="-DNDEBUG -ffile-prefix-map=$target_dir=/cmg-target"
# mise exports RUSTUP_TOOLCHAIN=stable; the explicit `cargo +<toolchain>`
# below must win so the artifact does not depend on the host's stable.
unset RUSTUP_TOOLCHAIN

cd "$crate_dir"
cargo "+$toolchain" build --locked --target wasm32-wasip1 --release --target-dir "$target_dir"
cp "$target_dir/wasm32-wasip1/release/code_mode_guest.wasm" "$out"
