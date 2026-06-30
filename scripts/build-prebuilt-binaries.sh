#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT"

HOST_TARGET="$(rustc -vV | awk '/^host:/ { print $2 }')"
COMMON_TARGETS="x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin"
STRICT="${REFLEX_PREBUILD_STRICT:-0}"

if [ "${REFLEX_PREBUILD_TARGETS:-}" ]; then
  TARGETS="$REFLEX_PREBUILD_TARGETS"
else
  TARGETS=""
  if command -v rustup >/dev/null 2>&1; then
    installed_targets="$(rustup target list --installed | awk '{ print $1 }')"
    for target in $COMMON_TARGETS; do
      if printf '%s\n' "$installed_targets" | grep -qx "$target"; then
        TARGETS="$TARGETS $target"
      fi
    done
  fi
  case " $TARGETS " in
    *" $HOST_TARGET "*) ;;
    *) TARGETS="$TARGETS $HOST_TARGET" ;;
  esac
fi

for target in $TARGETS; do
  case "$target" in
    *windows*) exe=".exe" ;;
    *) exe="" ;;
  esac

  if command -v rustup >/dev/null 2>&1; then
    if ! rustup target list --installed | awk '{ print $1 }' | grep -qx "$target"; then
      echo "missing Rust target: $target" >&2
      echo "install it with: rustup target add $target" >&2
      if [ "$STRICT" = "1" ]; then
        exit 1
      fi
      continue
    fi
  fi

  case "$(uname -s 2>/dev/null)-$target" in
    Darwin-*) cargo build --release --target "$target" --bins ;;
    *-*-apple-darwin)
      if ! command -v cargo-zigbuild >/dev/null 2>&1; then
        echo "cargo-zigbuild is required to build $target from non-macOS hosts" >&2
        echo "install it with: cargo install cargo-zigbuild --locked" >&2
        exit 1
      fi
      if ! command -v zig >/dev/null 2>&1; then
        echo "zig is required to build $target from non-macOS hosts" >&2
        echo "install Zig and ensure the zig binary is on PATH" >&2
        exit 1
      fi
      cargo zigbuild --release --target "$target" --bins
      ;;
    *) cargo build --release --target "$target" --bins ;;
  esac

  out_dir="bin/prebuilt/$target"
  mkdir -p "$out_dir"
  cp "target/$target/release/reflex$exe" "$out_dir/reflex$exe"
  cp "target/$target/release/reflex-mcp$exe" "$out_dir/reflex-mcp$exe"
  chmod +x "$out_dir/reflex$exe" "$out_dir/reflex-mcp$exe"
done

if [ "$STRICT" = "1" ]; then
  for target in $COMMON_TARGETS; do
    case " $TARGETS " in
      *" $target "*) ;;
      *)
        echo "strict prebuild mode did not include common target: $target" >&2
        echo "set REFLEX_PREBUILD_TARGETS to include every public release target" >&2
        exit 1
        ;;
    esac
  done
fi

echo "prebuilt Reflex binaries refreshed for:$TARGETS"
