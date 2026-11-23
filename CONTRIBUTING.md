# Contributing

Thanks for helping build Developer Workbench.

## Before you start

- For anything larger than a small fix, open an issue first so we can agree on the approach.
- Read the [product spec](docs/developer_workbench_product_spec.md). MVP scope is API Studio only;
  other modules are roadmap items.

## Workflow

1. Fork and create a branch from `main`.
2. Make the change with tests. Core business logic targets 80% coverage.
3. Run the checks:

   ```sh
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   pnpm lint && pnpm typecheck
   ```

   Storage changes must pass on PostgreSQL too: set `WORKBENCH_TEST_POSTGRES_URL`
   (see the README) or rely on the `Storage (PostgreSQL)` CI job. Schema changes
   need a new migration in **both** `migrations/sqlite` and `migrations/postgres`.

4. Open a pull request describing what changed and why.

## Code conventions

- **Crate boundaries:** `mock-engine` stays pure (no I/O, no async). Networking lives in
  `mock-runtime`; persistence in `workbench-storage`.
- **Errors:** `thiserror` in libraries, `anyhow` only in binaries. HTTP errors use the shared
  `ErrorEnvelope` from `workbench-core`.
- **No unsafe code** (`unsafe_code = "forbid"` workspace-wide).
- **Safe defaults:** loopback binding, redaction on, size and time limits on.
- **No user code execution:** response behaviour is declarative only.
- New dependencies: pin the version and check license and maintenance status.

## Commit messages

Use a short imperative subject (for example `feat(mock-engine): add contains operator`) and
explain the reason in the body when it is not obvious.

## Code of conduct

Participation is governed by our [Code of Conduct](CODE_OF_CONDUCT.md).
