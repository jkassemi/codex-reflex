# Privacy

Reflex stores local operational repair memory only. It does not sync data and does not store full raw tool output by default.

Redaction runs before storage and before lesson registration:

- authorization headers
- inline bearer tokens
- cookies
- passwords
- secret/token/API key fields
- AWS secret access keys
- AWS access key/session token assignment names
- API key and auth-token headers
- one-time URL parameters such as `access_token`, `code`, `sig`, and `client_secret`
- private key material
- GitHub token-shaped values

Stored tool output is summarized into exit code plus bounded stdout/stderr excerpts. The conformance suite includes a secrets-redaction demo that verifies raw bearer tokens and AWS secret values are not persisted.
