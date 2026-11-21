# crates/server

Rust server: axum 0.8, tokio, rusqlite (bundled SQLite with FTS5), argon2, rust-embed.

## Commands

- `cargo test -p earful` (unit tests in `src/`, HTTP tests in `tests/`)
- `cargo clippy -p earful --all-targets -- -D warnings`
- `cargo run -p earful -- serve` (reads `EARFUL_*` env, see `src/config.rs`)
- `printf '%s' pw | cargo run -q -p earful -- hash-password`
- `cargo run -q -p earful -- solve <challenge> <bits>`
- `cargo run -q -p earful -- download-model` (Model2Vec into `models/potion-base-8M`, gitignored)
- `EARFUL_TEST_MODEL2VEC_PATH=$PWD/models/potion-base-8M cargo test --release --test model2vec -- --nocapture` (real-model quality checks, skipped when unset)

## Gotchas

- Release builds embed `../../apps/admin/dist` and `../../packages/widget/dist` at compile time (`allow_missing`, so a missing build means an empty UI). Run `pnpm build:web` first. Debug builds read them from disk.
- `Config::for_tests` gives low PoW bits, `min_fill_secs = 0`, trusted proxy hops 1 (tests set `X-Forwarded-For` to simulate IPs) and the `hash` embedder.
- argon2 and sha2 are optimised in the dev profile (root `Cargo.toml`); keep that or tests get slow.
- New config: add it to `config.rs`, `.env.example` and the README table.
- New table or column: append a migration in `db.rs`. Remember FK cascades and the FTS triggers.
- Handlers return `AppResult<T>`. Map validation to `AppError::bad("invalid_<field>", ...)`.
- Vectors in `VectorIndex` must stay in sync with the `embeddings` table (insert, delete, project delete, reindex).
- Public submit must not leak outcome: silent drops and quarantines still return `202`.
- MCP (`src/mcp/`): add a tool by adding a `ToolDef` in `tools.rs` and a match arm in `run` that calls an `op_*` from `routes/admin.rs`. Return validation problems as `Err(String)` starting with the error code (they become `isError` results), and cover it in `tests/mcp.rs`.
- `EARFUL_MCP_URL=http://127.0.0.1:8080/mcp EARFUL_MCP_TOKEN=eat_... cargo run -q -p earful -- mcp-stdio` exercises the stdio bridge.
