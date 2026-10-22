# AGENTS.md

Self-hosted feedback and bug report collector: one Rust binary (axum + SQLite) with an embedded Preact admin UI and an embeddable widget. Single admin, anonymous public submissions guarded by layered anti-spam.

## Repo map

- `crates/server/` Rust server, binary `feedback-harvest` (`serve`, `hash-password`, `healthcheck`, `solve`, `mcp-stdio`)
  - `src/routes/` HTTP handlers (`public.rs` ingestion + hosted form, `admin.rs`, `assets.rs`)
  - `src/mcp/` MCP server (`mod.rs` JSON-RPC transport, `tools.rs`, `resources.rs` resources + prompts, `bridge.rs` stdio bridge), `src/tokens.rs` API tokens
  - `src/db.rs` schema + migrations, `src/pow.rs` proof of work, `src/spam.rs` heuristics + client IP, `src/search.rs` keyword/fuzzy/RRF, `src/embed.rs` embedders + vector index, `src/worker.rs` background jobs
  - `tests/` HTTP integration tests
- `apps/admin/` admin SPA (Preact, Vite), built to `dist/`, served at `/admin/`
- `packages/widget/` browser client, PoW solver and `widget.js` (also used by `/s/<slug>`)
- `e2e/` Playwright tests against the real binary
- `docs/api.md` HTTP contract (source of truth), `docs/mcp.md` MCP guide, `docs/architecture.md`, `docs/deploy-coolify.md`, `docs/integrations.md`

## Setup

- Rust stable via rustup (`rust-toolchain.toml` pins components), Node 22+, pnpm via `corepack enable`.
- `pnpm install` at the repo root. Never use npm or yarn.

## Commands

| Task | Command |
| --- | --- |
| Build web assets | `pnpm build:web` (widget, then admin) |
| Release build | `pnpm build` (web first: release builds embed `dist/` at compile time) |
| Run server | `pnpm dev:server` (needs `FH_ADMIN_PASSWORD_HASH`, `FH_COOKIE_SECURE=false` for http) |
| Admin dev server | `pnpm dev:admin` (proxies `/api` to `127.0.0.1:8080`) |
| Rust tests | `cargo test --workspace` or `cargo test -p feedback-harvest <name>` |
| Rust lint | `pnpm lint:rust` (rustfmt check + clippy `-D warnings`) |
| Web tests | `pnpm test:web`, or `pnpm --filter @feedback-harvest/widget test` |
| Typecheck | `pnpm typecheck` |
| E2E | `pnpm test:e2e` (first time: `pnpm --filter @feedback-harvest/e2e exec playwright install chromium`) |
| Everything | `pnpm check` (run before declaring work done) |

## Invariants (do not break)

- `POST /api/v1/feedback` answers `202 {"ok": true}` for every silent outcome: stored, merged duplicate, quarantined, dropped. Only validation, rate limit and challenge failures return errors.
- Soft spam signals quarantine (`status = 'spam'` + `spam_reason`), they never reject.
- Raw IP addresses are never stored or logged. Use `AppState::ip_hash`.
- Every `/api/admin/*` handler except `login` takes the `Admin` extractor. No exceptions.
- Default bind is `127.0.0.1`. Only the Docker image sets `0.0.0.0` (in-container, never published publicly). Compose publishes to `127.0.0.1` only.
- Migrations in `db.rs` are append-only. Never edit a shipped migration; add a new entry.
- FTS5 tables `feedback_fts` and `feedback_tri` are kept in sync by triggers. Write to `feedback` only, never to the FTS tables directly.
- The PoW algorithm is implemented in Rust (`pow.rs`) and TypeScript (`packages/widget`). Both test the same vector from `docs/api.md`; change both together.
- `docs/api.md` is the contract. Update it in the same change as any API change.
- Without `FH_ADMIN_PASSWORD_HASH`, every login fails. Keep auth fail-closed.
- MCP tools (`src/mcp/tools.rs`) call the shared `op_*` functions in `routes/admin.rs` (and `public::op_create_feedback`). Never duplicate SQL or validation in MCP code; extract a shared op instead.
- Every MCP write tool sets `write: true` and is therefore hidden from and refused for `read` tokens. Destructive tools require a confirmation argument and `destructive: true`.
- `/mcp` always requires a valid bearer token (`tokens::authenticate`). API tokens are stored only as SHA-256; never return or log raw tokens after creation.

## Conventions

- Rust: `cargo fmt`, clippy clean with `-D warnings`. Return `AppError` from handlers (stable `error` codes). All SQL goes through `Db::call` (runs on the blocking pool). Use `params![]`, never format SQL with user input.
- TypeScript: `strict` mode. Never assign user or server data to `innerHTML`; use `textContent` or Preact rendering. No external CDNs, fonts or scripts: the admin CSP is `self` only.
- Keep dependencies few. Justify any new crate or npm package.

## Testing expectations

- Every endpoint change gets an integration test in `crates/server/tests/`.
- Every MCP tool, resource and prompt has coverage in `crates/server/tests/mcp.rs` (happy path and validation errors).
- Every anti-spam or search rule change gets a unit test next to the code.
- UI flows that matter (login, project CRUD, submit, search, triage) are covered in `e2e/`.

## Security

- Never commit secrets, `.env` or `*.db` files. Hashes and API keys come from env vars or `*_FILE` variants.
- Ingest keys are public by design; do not treat them as credentials.
- Admin cookie stays `HttpOnly`, `SameSite=Strict`, `Secure` by default.

## Commits

- Small, focused commits with imperative subject lines ("Add bulk status endpoint").
- Run `pnpm check` (or at least the affected package's tests and lints) before committing.

## Writing style

- Never use em dashes in code, comments, docs or commit messages. Use commas, colons, parentheses or separate sentences.
