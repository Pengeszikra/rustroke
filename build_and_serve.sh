#!/usr/bin/env bash
set -euo pipefail

# Build the WASM, copy it to the web folder, then serve the page.

cd "$(dirname "$0")"

# Ensure cargo from rustup is picked up
export PATH="$HOME/.cargo/bin:$PATH"

rustup +nightly target add wasm32-unknown-unknown >/dev/null
cargo +nightly build --release --target wasm32-unknown-unknown

cp target/wasm32-unknown-unknown/release/rust_svg_editor.wasm web/rust-svg-editor.wasm

serve -port 8080 ./web

