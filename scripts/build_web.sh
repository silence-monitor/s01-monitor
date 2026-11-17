#!/bin/bash
# Builds the browser version of the monitor into web/dist.
#   needs: rustup target add wasm32-unknown-unknown
#          cargo install wasm-bindgen-cli --version 0.2.129   (match Cargo.lock)
#   optional: wasm-opt (binaryen) for a further ~15% off the download
set -euo pipefail
cd "$(dirname "$0")/.."
# Paths a program remembers: replace the home folder and the project folder,
# so the page (and the ESP32 image that carries it) names neither.
export RUSTFLAGS="--remap-path-prefix=$HOME=/build --remap-path-prefix=$PWD=/src ${RUSTFLAGS:-}"
cargo build --profile web --target wasm32-unknown-unknown -p s01-monitor
rm -rf web/dist && mkdir -p web/dist
wasm-bindgen --target web --no-typescript --out-dir web/dist \
  target/wasm32-unknown-unknown/web/s01-monitor.wasm
if command -v wasm-opt >/dev/null; then
  wasm-opt -Oz --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int \
    --enable-simd --enable-mutable-globals --enable-reference-types \
    web/dist/s01-monitor_bg.wasm -o web/dist/s01-monitor_bg.wasm
fi
cp web/index.html web/dist/
gzip -9 -k -f web/dist/s01-monitor_bg.wasm
ls -l web/dist
