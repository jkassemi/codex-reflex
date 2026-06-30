# Reflex

**Note: Store preferences and lessons with your project in plain markdown. This is just an exploration of tool call hooks in codex.**

Operational repair memory for Codex tool calls.

Reflex watches failed tool calls, lets Codex register what actually fixed them, stores concise scoped lessons locally, and helps Codex avoid repeating similar operational mistakes. When a future command violates a required stored repair, Reflex blocks that command and returns the corrected invocation; otherwise it can provide a concise hint.

## Quick Install

```sh
codex plugin marketplace add jkassemi/codex-reflex
codex plugin add reflex@codex-reflex
```

Then start a new Codex session and review and trust the bundled Reflex hooks.

## How it Works

Before:

```
pytest tests/test_auth.py
Command 'pytest' not found
```

After Reflex Learns:

```
feedback: Reflex blocked this command because it violates a stored operational repair. Run `PYTHONPATH=/repo ./.venv/bin/pytest` instead.
```

It learns operational fixes like:

- Wrong working directory
- Missing package manager prefixes (uv, run, cargo, etc)
- AWS profile/region flags
- Permission patterns
- MCP argument shapes

Reflex is intentionally not general memory.

It does not store secrets and does not rewrite commands. The blocked command is rejected, and Codex decides whether to run the supplied replacement.

## MCP Tools

The bundled MCP server name is `reflex`. Codex should register a repair only after a repair succeeds or the user confirms the fix. Codex should not register a new lesson when Reflex already blocked a command and supplied the repair. Lessons are candidates until confirmed by later use, and the MCP surface supports registration, retrieval, feedback, case inspection, and disabling bad lessons.


## CLI

Data is stored in `PLUGIN_DATA` when Codex provides it, then `REFLEX_DATA`, then `~/.local/share/reflex`.

Use `bin/reflex doctor` when validating install and hook trust, `bin/reflex status` to inspect storage size, and `bin/reflex purge --keep-recent N` to keep bounded local history. Purge removes orphaned unresolved cases/injections, checkpoints the SQLite WAL, and vacuums the database.


## Development

Public release packages include prebuilt `reflex` and `reflex-mcp` binaries for the user's platform. Users should not need to run `cargo build` after installing a release package.

Local developer install:

```sh
scripts/build-prebuilt-binaries.sh
codex plugin marketplace add .
codex plugin add reflex@codex-reflex
```

For local development in this repository, run `scripts/local-release-gate.sh` before sharing changes. The tracked pre-commit hook runs the same local gate; install it into a checkout with:

```sh
cp .githooks/pre-commit .git/hooks/pre-commit
```

The prebuild script targets Linux x86_64, Linux ARM64, macOS Intel, and macOS Apple Silicon by default, building the subset installed on the developer machine. Before publishing, run strict mode with every public release target configured:

```sh
REFLEX_PREBUILD_STRICT=1 REFLEX_PREBUILD_TARGETS="x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin" scripts/build-prebuilt-binaries.sh
```

## Validate

```sh
cargo fmt --check
cargo test
python3 /home/james/.codex/skills/.system/plugin-creator/scripts/validate_plugin.py .
```

The tests cover wrong-cwd hints, package-manager hints, required-environment command blocks, explicit MCP registration, CLI listing, MCP tool visibility, secrets redaction, storage purge behavior, lesson confidence updates, and avoiding lessons from ordinary product-code fixes.
