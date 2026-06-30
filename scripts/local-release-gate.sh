#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT"

cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings

validator="${REFLEX_PLUGIN_VALIDATOR:-$HOME/.codex/skills/.system/plugin-creator/scripts/validate_plugin.py}"
if [ ! -f "$validator" ]; then
  echo "plugin validator not found: $validator" >&2
  echo "set REFLEX_PLUGIN_VALIDATOR to plugin-creator/scripts/validate_plugin.py" >&2
  exit 1
fi

python3 "$validator" .
scripts/build-prebuilt-binaries.sh
scripts/validate-plugin-install.sh
