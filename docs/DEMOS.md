# Demos

The automated conformance demos live in `tests/conformance/reflex_demos.rs` and are executed through `tests/reflex_demos.rs`.

Run:

```sh
cargo test --test reflex_demos
```

Covered scenarios:

- wrong working directory: root `pytest` failure, `services/api` + `uv run pytest` success, future root command gets a hint
- package manager substitution: `npm test` failure, `pnpm test` success, future `npm test` gets a hint
- explicit registration: MCP registration stores a lesson that the CLI lists
- secrets redaction: bearer tokens and AWS secret values are not stored
- code-fix non-learning: a product-code patch followed by passing tests does not create an operational lesson without explicit registration
- operational maintenance: purge keeps a bounded recent attempt history, lesson confirmations/contradictions update state, and unresolved Stop does not promote open cases
