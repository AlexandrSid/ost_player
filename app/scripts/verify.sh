#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

fix_fmt="${1:-}"

if [[ "${fix_fmt}" == "--fix-fmt" ]]; then
  cargo fmt --all
else
  cargo fmt --all -- --check
fi

cargo clippy --all-targets --all-features -- -D warnings
cargo test --all

echo "All checks passed."

