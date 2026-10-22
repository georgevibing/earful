# Contributing

Thanks for taking the time. This is a small personal project, so the process is light.

## Setup

- Rust stable via [rustup](https://rustup.rs) (components are pinned in `rust-toolchain.toml`)
- Node 22+ and pnpm (`corepack enable`). Do not use npm or yarn.

```sh
pnpm install
pnpm build:web
cargo build
pnpm --filter @feedback-harvest/e2e exec playwright install chromium   # first time only
```

See the Development section of the [README](README.md#development) for running the server and the admin UI locally.

## Before you open a pull request

Run the full gate and make sure it is green:

```sh
pnpm check
```

It covers typecheck, rustfmt, clippy with `-D warnings`, Rust unit and integration tests, widget and admin tests, and the Playwright suite against the real binary.

## Conventions

[AGENTS.md](AGENTS.md) is the single source for conventions and invariants, for humans and coding agents alike. The short version:

- `docs/api.md` is the HTTP contract. Change it in the same commit as the API.
- Every endpoint change gets an integration test in `crates/server/tests/`, every anti-spam or search rule a unit test next to the code.
- Migrations in `crates/server/src/db.rs` are append-only.
- The proof of work exists in Rust and TypeScript and both test the same vector. Change them together.
- No `innerHTML` with user or server data, no external CDNs (the admin CSP is `self` only).
- No em dashes in code, comments, docs or commit messages.

## Commits

Small, focused commits with an imperative subject line, for example `Add webhook notifications for new bugs`. Explain the why in the body when it is not obvious.

## Screenshots

If you change the UI, regenerate the README screenshots:

```sh
pnpm screenshots
```
