# Explicit Repair Registration Demo

Scenario:

Codex recognizes the reusable operational fix after a command succeeds and calls:

```text
mcp__reflex__register_repair_episode
```

Expected Reflex behavior:

- A candidate lesson is stored.
- `reflex lessons` lists the lesson.
- `PreToolUse` injects the lesson only on a close future match.

The automated version is `explicit_registration_stores_lesson_and_cli_lists_it` in `tests/conformance/reflex_demos.rs`.
