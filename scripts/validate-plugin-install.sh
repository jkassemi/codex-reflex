#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

if ! command -v codex >/dev/null 2>&1; then
  echo "codex CLI is required for plugin install validation" >&2
  exit 1
fi

home_parent="${REFLEX_CODEX_TEST_HOME_PARENT:-${XDG_CACHE_HOME:-$HOME/.cache}/codex-reflex}"
mkdir -p "$home_parent"
tmp_home="$(mktemp -d "$home_parent/reflex-codex-home.XXXXXX")"
cleanup() {
  rm -rf "$tmp_home"
}
trap cleanup EXIT HUP INT TERM

CODEX_HOME="$tmp_home" codex plugin marketplace add "$ROOT" --json >/dev/null
CODEX_HOME="$tmp_home" codex plugin add reflex@codex-reflex --json >/dev/null

mcp_config="$(CODEX_HOME="$tmp_home" codex mcp get reflex)"

case "$mcp_config" in
  *'${PLUGIN_ROOT}'*)
    echo "plugin MCP command still contains unsupported \${PLUGIN_ROOT} interpolation" >&2
    echo "$mcp_config" >&2
    exit 1
    ;;
esac

case "$mcp_config" in
  *"command: ./bin/reflex-mcp"*) ;;
  *)
    echo "plugin MCP command must launch the packaged wrapper from the installed plugin root" >&2
    echo "$mcp_config" >&2
    exit 1
    ;;
esac

case "$mcp_config" in
  *"cwd: $tmp_home/plugins/cache/codex-reflex/reflex/"*) ;;
  *)
    echo "plugin MCP cwd must resolve inside the installed plugin cache" >&2
    echo "$mcp_config" >&2
    exit 1
    ;;
esac

installed_root="$(find "$tmp_home/plugins/cache/codex-reflex/reflex" -mindepth 1 -maxdepth 1 -type d | sort | tail -n 1)"

status="$(CODEX_HOME="$tmp_home" "$installed_root/bin/reflex" status)"
case "$status" in
  *"Reflex data: $tmp_home/plugins/data/reflex-codex-reflex"*) ;;
  *)
    echo "installed wrapper must default to the Codex plugin data directory" >&2
    echo "$status" >&2
    exit 1
    ;;
esac

CODEX_HOME="$tmp_home" "$installed_root/bin/reflex" doctor >/dev/null
CODEX_HOME="$tmp_home" "$installed_root/bin/reflex-mcp" --help >/dev/null

echo "plugin install validation passed"
