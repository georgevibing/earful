# feedback-harvest

A tiny self-hosted service for collecting bug reports and feedback from anything you ship: a Steam game, a web app, a CLI, a backend service. Users submit anonymously (no accounts), layered anti-spam keeps the inbox clean, and a small password-protected admin UI lets the single admin manage projects and dig through submissions with fuzzy, keyword, semantic and hybrid search. It is one Rust binary plus one SQLite file, built for a Coolify box, not for SaaS.

## Features

| Requirement | What you get |
| --- | --- |
| Project CRUD | Create, edit, pause and delete projects (title, slug, type, description). Each project has a public, rotatable ingest key. |
| Anonymous submission | `POST /api/v1/feedback` for bugs and feedback with optional title, contact, 1 to 5 rating and free-form `meta` JSON (version, platform, build). |
| Hosted form and widget | `/s/<slug>` hosted feedback page and an embeddable `/widget.js` for web products. |
| Layered anti-spam | Per-project keys, sliding window rate limits, adaptive proof of work, single-use signed challenges, honeypot, minimum fill time, exact duplicate merging, heuristic spam scoring, near duplicate detection. Suspicious items are quarantined, not rejected. |
| Search | Fuzzy find (trigram + typo tolerant edit distance), keyword (FTS5 BM25 with stemming), semantic (embeddings) and hybrid (reciprocal rank fusion of all three). "Similar items" on every detail view. |
| Triage | Statuses `new, triaged, resolved, archived, spam`, admin notes, bulk actions. |
| Dashboard | Per-project counts, 30 day activity, blocked spam by reason, embedding status. |
| Export | JSON or CSV per project. |
| Small footprint | Single static-ish binary with the admin UI embedded, SQLite in WAL mode, no other services required. |

## Architecture

```
  game / app / site                      you (single admin)
        |                                      |
        | GET  /api/v1/challenge               | /admin  (session cookie)
        | POST /api/v1/feedback                | /api/admin/*
        v                                      v
 +----------------------------------------------------------+
 |              feedback-harvest (one Rust binary)          |
 |                                                          |
 |  axum HTTP  ->  anti-spam pipeline  ->  SQLite (WAL)     |
 |                                         |- feedback      |
 |  embedded admin UI + widget.js          |- FTS5 porter   |
 |                                         |- FTS5 trigram  |
 |  background worker  --embeds-->         '- embeddings    |
 |        |                     in-memory vector index      |
 +--------|-------------------------------------------------+
          v (optional)
   OpenAI compatible /embeddings  (OpenAI, Ollama, LM Studio, ...)
```

Details in [docs/architecture.md](docs/architecture.md). The HTTP contract lives in [docs/api.md](docs/api.md).

## Repository layout

```
crates/server/      Rust server (axum, rusqlite, argon2). Binary: feedback-harvest
apps/admin/         Admin UI (Vite), built to dist/ and embedded into the binary at /admin/
packages/widget/    Browser client + embeddable widget (widget.js), PoW solver in TypeScript
e2e/                Playwright end to end tests against the real binary
docs/               API contract, architecture, deployment, integrations
```

pnpm workspace for the TypeScript packages, Cargo workspace for Rust.

## Quickstart

Prerequisites: Rust (stable, via rustup), Node 22+ and pnpm (`corepack enable`).

```sh
pnpm install

# 1. Create an admin password hash (reads the password from stdin)
printf '%s' 'choose-a-long-password' | cargo run -q -p feedback-harvest -- hash-password
```

### Local development (two terminals)

```sh
# Terminal 1: API server on http://127.0.0.1:8080
export FH_ADMIN_PASSWORD_HASH='<hash from step 1>'   # single quotes: the hash contains $
export FH_COOKIE_SECURE=false                         # plain http on localhost only
pnpm dev:server

# Terminal 2: admin UI with hot reload, proxies /api to the server
pnpm dev:admin
```

Open http://127.0.0.1:5173/admin/ and log in. To try the embedded build instead, run `pnpm build:web` and open http://127.0.0.1:8080/admin/ (debug builds read the web assets from disk, release builds embed them).

### Docker Compose

```sh
cp .env.example .env
# put the hash into .env inside single quotes: FH_ADMIN_PASSWORD_HASH='$argon2id$v=19$...'
docker compose up -d --build
open http://127.0.0.1:8080/admin/
```

The compose file publishes only on `127.0.0.1`. To reach it from another machine, use an SSH tunnel (`ssh -L 8080:127.0.0.1:8080 host`) or deploy behind Coolify, see [docs/deploy-coolify.md](docs/deploy-coolify.md).

## Configuration

All settings are environment variables. See [.env.example](.env.example) for a commented template.

| Variable | Default | Purpose |
| --- | --- | --- |
| `FH_BIND` | `127.0.0.1:8080` | Listen address. The Docker image sets `0.0.0.0:8080` inside the container. |
| `FH_DATABASE_PATH` | `feedback-harvest.db` | SQLite file. Image default `/data/feedback-harvest.db`. |
| `FH_ADMIN_PASSWORD_HASH` | unset | Argon2 PHC string. Unset means every login is rejected. |
| `FH_ADMIN_PASSWORD_HASH_FILE` | unset | File containing the hash. Takes precedence. |
| `FH_SESSION_TTL_HOURS` | `168` | Admin session lifetime. |
| `FH_COOKIE_SECURE` | `true` | `Secure` flag on the session cookie. Only disable for http on localhost. |
| `FH_TRUSTED_PROXY_HOPS` | `0` | Trusted reverse proxies for `X-Forwarded-For`. Image default `1`. |
| `FH_LIMIT_IP_PROJECT_MINUTE` | `3` | Submissions per IP per project per minute. |
| `FH_LIMIT_IP_PROJECT_HOUR` | `10` | Per IP per project per hour. |
| `FH_LIMIT_IP_PROJECT_DAY` | `30` | Per IP per project per day. |
| `FH_LIMIT_IP_GLOBAL_DAY` | `60` | Per IP across all projects per day. |
| `FH_LIMIT_PROJECT_HOUR` | `300` | Per project across all IPs per hour (flood guard). |
| `FH_LIMIT_CHALLENGES_IP_HOUR` | `60` | Challenges issued per IP per hour. |
| `FH_MIN_FILL_SECS` | `2` | Submissions faster than this after the challenge are quarantined. |
| `FH_SPAM_THRESHOLD` | `0.7` | Heuristic score that sends a submission to the spam bucket. |
| `FH_POW_BITS` | `18` | Base proof of work difficulty (leading zero bits). |
| `FH_POW_MAX_BITS` | `23` | Upper bound for adaptive difficulty. |
| `FH_CHALLENGE_TTL_SECS` | `1800` | Challenge lifetime. |
| `FH_EMBED_PROVIDER` | `hash` | `none`, `hash` or `openai`. |
| `FH_EMBED_DIM` | `384` | Vector size for the `hash` provider. |
| `FH_EMBED_URL` | `https://api.openai.com/v1` | Base URL of an OpenAI compatible API. |
| `FH_EMBED_MODEL` | `text-embedding-3-small` | Embedding model name. |
| `FH_EMBED_API_KEY` / `FH_EMBED_API_KEY_FILE` | unset | Bearer token for the embedding API. |
| `RUST_LOG` | `info` | Log filter (tracing `EnvFilter` syntax). |

Login attempts are limited to 5 per 15 minutes per IP (not configurable).

## Embedding providers

Semantic search quality depends entirely on the embedding provider. Switching providers is safe: vectors are tagged with the model id, and rows embedded with another model are re-embedded in the background.

- **`hash` (default)**: built in, offline, deterministic, zero cost. Signed feature hashing over stemmed words, bigrams and character trigrams, plus a small concept map that groups common game and app vocabulary (crash, freeze, stuck; lag, stutter, fps; and so on). It is lexical with a little help, not real semantic understanding. Good enough to get started and for tests.
- **`openai`** with OpenAI: `FH_EMBED_URL=https://api.openai.com/v1`, `FH_EMBED_MODEL=text-embedding-3-small`, `FH_EMBED_API_KEY=...`. Excellent quality, costs fractions of a cent at this volume, sends feedback text to OpenAI.
- **`openai`** with Ollama (self-hosted, recommended for real semantic search without third parties): run Ollama next to the app (a Coolify service or the commented sidecar in `docker-compose.yml`), `ollama pull nomic-embed-text`, then `FH_EMBED_URL=http://ollama:11434/v1`, `FH_EMBED_MODEL=nomic-embed-text`. Any other OpenAI compatible server (LM Studio, vLLM, llama.cpp server) works the same way.

If the provider is down, submissions still land and keyword/fuzzy search keep working. The worker retries with backoff and fills in vectors later. `POST /api/admin/reindex` re-embeds everything.

## Integrating a product

1. Create a project in the admin UI and copy its ingest key. The key is public by design (it ships inside your game or page) and can be rotated any time.
2. Pick an integration:
   - Link to the hosted page `https://<your-host>/s/<slug>`.
   - Embed the widget on a web page, see [packages/widget/README.md](packages/widget/README.md).
   - Call the API from any client: fetch a challenge, solve the proof of work, submit. See [docs/integrations.md](docs/integrations.md) for curl, Unity (C#), Godot (GDScript), C++ and Python examples.

## Backups

The whole state is one SQLite database in WAL mode. Do not just copy the `.db` file while the server runs (recent writes may still sit in the `-wal` file). Instead:

- Online snapshot from the host: `sqlite3 /path/to/volume/feedback-harvest.db ".backup '/backups/fh-$(date +%F).db'"`, or `VACUUM INTO '/backups/fh.db'`.
- Continuous replication to S3 compatible storage with [Litestream](https://litestream.io) running next to the app against the same volume.
- Or stop the container and copy the `.db`, `-wal` and `-shm` files together.

## Security model

- Single admin. The password is checked with argon2id; sessions are random 256 bit tokens, only their SHA-256 is stored. The cookie is `HttpOnly`, `SameSite=Strict` and `Secure` by default. State changing admin requests with a foreign `Origin` are rejected. Without a configured hash, every login fails.
- The public ingestion API is intentionally unauthenticated (anonymous end users are its audience) and is protected by the anti-spam layers instead. It always answers `202 {"ok": true}` for stored, merged, quarantined and dropped submissions, so spammers learn nothing.
- Raw IP addresses are never stored, only keyed HMAC pseudonyms (IPv6 at /64 granularity).
- Private by default: the server binds `127.0.0.1` unless told otherwise. The Docker image binds `0.0.0.0` inside the container, which is only acceptable because Coolify publishes no host port (its proxy reaches the container over the private Docker network) and the compose file publishes to `127.0.0.1` only. Never publish port 8080 on a public interface; expose the app through the HTTPS proxy.
- Strict security headers and a `self`-only Content Security Policy on the admin UI.

## Testing

```sh
pnpm check        # typecheck, rustfmt, clippy, Rust tests, web tests, Playwright e2e
pnpm test:rust    # cargo test --workspace
pnpm test:web     # widget and admin unit tests
pnpm test:e2e     # builds web + server, then runs Playwright against the real binary
```

## License

MIT, see [LICENSE](LICENSE).
