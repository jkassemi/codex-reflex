# Secrets Redaction Demo

Scenario:

```text
Authorization: Bearer abc123
AWS_SECRET_ACCESS_KEY=supersecret
```

Expected Reflex behavior:

- Raw bearer tokens and AWS secret values are redacted before storage.
- SQLite storage contains `[REDACTED]`, not the original secret value.

The automated version is `secrets_redaction_demo_does_not_store_raw_secrets` in `tests/conformance/reflex_demos.rs`.
