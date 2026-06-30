# Reflex Architecture

Reflex separates observation, interpretation, and future application.

Hooks observe supported Codex tool events and maintain the local repair episode timeline. The model-facing MCP surface is the interpretation path: Codex registers a lesson only after it knows what repair actually worked. Future application happens through bounded local retrieval before similar tool calls, where Reflex injects concise hints rather than rewriting commands.

All durable state is local SQLite under `PLUGIN_DATA` or `REFLEX_DATA`. Redaction happens before storage and before lesson registration. The CLI exists for operational inspection, trust/debug workflows, export, and bounded retention.

The hot hook path is synchronous and bounded. The optional analyzer is represented by `reflex analyze`, but model-backed automatic analysis is disabled by default.

Operational posture:

- SQLite opens with WAL, normal synchronous mode, foreign keys, and a bounded busy timeout.
- The schema includes indexes for session/project/status lookups used by hooks and retrieval.
- `reflex status` and `reflex doctor` expose storage counts and byte size.
- `reflex purge --keep-recent N` bounds attempt history and vacuums storage.
- Hook wrappers require prebuilt binaries and fail fast if `cargo build --release` has not run.
- `Stop` does not infer a repair for unresolved open cases; explicit MCP registration remains the high-quality path.

Validation:

```sh
cargo fmt --check
cargo test
python3 /home/james/.codex/skills/.system/plugin-creator/scripts/validate_plugin.py .
```
