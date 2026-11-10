#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked -p glassftp-web --target wasm32-unknown-unknown --release
mkdir -p dist/pkg
if [ -x .tools/bin/wasm-bindgen ]; then
  .tools/bin/wasm-bindgen --target web --out-dir dist/pkg target/wasm32-unknown-unknown/release/glassftp_web.wasm
else
  wasm-bindgen --target web --out-dir dist/pkg target/wasm32-unknown-unknown/release/glassftp_web.wasm
fi
cp assets/index.html assets/boot.js assets/bridge.js assets/style.css assets/favicon.svg dist/
