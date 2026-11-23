# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security problems. Use GitHub's
[private vulnerability reporting](https://github.com/maspandd/johto-dev-tools/security/advisories/new)
for this repository.

Include the affected version or commit, reproduction steps, and the impact you expect.
We aim to acknowledge reports within 5 working days.

## Supported versions

The project is pre-1.0. Security fixes are applied to `main`.

## Security defaults

- Mock servers bind to `127.0.0.1`; other interfaces require explicit opt-in.
- Authorization headers, cookies, API keys and common secret fields are redacted from request history.
- Request bodies and simulated delays are size- and time-limited.
- JSON Schemas never fetch remote or local `$ref` targets.
- No user-supplied scripts or native code are executed.
