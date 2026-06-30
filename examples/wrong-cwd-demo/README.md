# Wrong CWD Demo

Scenario:

```text
pytest tests/test_auth.py
  -> fails from the repository root

cd services/api && uv run pytest tests/test_auth.py
  -> succeeds
```

Expected Reflex behavior:

- `PostToolUse` records the root failure as a case.
- `register_repair_episode` stores a candidate lesson for running API tests from `services/api` with `uv run pytest`.
- A later root `pytest tests/test_auth.py` receives a `PreToolUse` hint.

The automated version is `wrong_cwd_demo_registers_and_injects_close_match` in `tests/conformance/reflex_demos.rs`.
