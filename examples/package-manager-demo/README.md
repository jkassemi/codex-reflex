# Package Manager Demo

Scenario:

```text
npm test
  -> fails

pnpm test
  -> succeeds
```

Expected Reflex behavior:

- The failed `npm test` opens a repair case.
- Explicit registration stores the lesson that this repo uses `pnpm test`.
- A later `npm test` receives a hint before execution.

The automated version is `package_manager_demo_injects_pnpm_hint_for_npm_test` in `tests/conformance/reflex_demos.rs`.
