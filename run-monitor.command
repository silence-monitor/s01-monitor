#!/bin/bash
# Launcher for the S01 monitor. Safe to delete.
cd "$(dirname "$0")" || exit 1
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust (cargo) is not installed on this Mac. Install it from https://rustup.rs and run this again."
  exit 1
fi
cargo run --release -p s01-monitor
