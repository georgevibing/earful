# Architecture

earful is deliberately boring: one process, one SQLite file, no queue, no cache server, no search cluster. Everything below fits in a few MB of RAM for tens of thousands of submissions.

## Components

| Component | Where | Role |
| --- | --- | --- |
| HTTP server | `crates/server` (axum, tokio) | Public ingestion API, hosted form, admin API, static assets. |
| Storage | SQLite via rusqlite (bundled, WAL mode) | Projects, feedback, labels, sessions, rate counters, used challenges, block events, embeddings. |
| Keyword index | FTS5 table `feedback_fts`, `porter unicode61` tokenizer | Stemmed word search ranked by BM25. |
| Fuzzy index | FTS5 table `feedback_tri`, `trigram` tokenizer | Substring and typo tolerant candidate retrieval. |
| Vector index | `embeddings` table + in-memory `VectorIndex` | Semantic search and "similar items". Brute force cosine. |
| Embedder | `embed.rs`, `download.rs` | `model2vec` (local static embeddings, default in Docker), `hash` (built in, lexical) or any OpenAI compatible `/embeddings` endpoint. `auto` picks Model2Vec when its model directory exists. |
| Background worker | `worker.rs` | Embedding backfill, near duplicate detection, hourly pruning. |
| Admin UI | `apps/admin`, embedded with rust-embed | Single page app served at `/admin/`. |
| Widget | `packages/widget`, embedded with rust-embed | `widget.js` for web pages and the hosted `/s/<slug>` page. |

`Db` wraps a single connection behind a mutex and runs every query on the blocking thread pool (`Db::call`). SQLite serialises writes anyway, and a single admin plus modest ingestion never comes close to its limits.

## Data model

```
projects        id, slug (unique), title, project_type, description,
                ingest_key (unique, public), accepting, created_at, updated_at
feedback        id, project_id -> projects (cascade), kind (bug|feedback|crash|exception|log),
                title, body, contact, rating (1..5), meta (JSON), source (api|web),
                status (new|triaged|resolved|archived|spam), spam_reason, spam_score,
                admin_note, content_hash, dup_count, ip_hash,
                created_at, updated_at, last_seen_at
labels          id, project_id -> projects (cascade), name, name_key (unique per project),
                color, description, created_at, updated_at
feedback_labels feedback_id, label_id, project_id (composite FKs enforce project scope)
feedback_fts    FTS5 external content over feedback(title, body, contact)
feedback_tri    FTS5 external content over feedback(title, body), trigram
embeddings      feedback_id -> feedback (cascade), model, vec (f32 LE blob)
sessions        token_hash (sha256), created_at, expires_at
rate_events     bucket, ip_hash, ts           sliding window counters
used_challenges id, expires_at                single use enforcement
block_counts    day, project_id, reason, n    per day dashboard counters (bounded growth)
api_tokens      name, scope, token_hash, prefix, last_used_at   MCP and script bearer tokens
settings        key, value                    server secret (HMAC key)
```

Triggers keep both FTS tables in sync on insert, update and delete, including cascaded deletes when a project is removed. Migrations are embedded in `db.rs`, append only, and tracked with `PRAGMA user_version`.

The server secret is generated on first boot and stored in `settings`. It keys the challenge signatures and the IP pseudonyms, so keep the database private and back it up as a unit.

## Submission pipeline

Every public submission passes these layers in order. Each one exists to make a specific attack expensive or pointless.

| # | Layer | Outcome on failure | Why |
| --- | --- | --- | --- |
| 1 | Ingest key lookup, `accepting` flag | `404` / `403 project_paused` | Scopes abuse to one project; a leaked key is rotated or the project paused without touching others. |
| 2 | Honeypot field `website` | Silent drop, `202` | Naive form fillers populate every field. |
| 3 | Validation (lengths, UTF-8, control chars, `meta` is an object <= 4 KB, body limit 32 KB) | `400 invalid_<field>` | Bounds storage and keeps payloads sane. Legit clients need the error. |
| 4 | Sliding window rate limits on hashed IP: per project per minute, hour, day; global per day; a per project hourly flood guard over all IPs (beyond it items are quarantined, at 3x refused); a total daily intake cap | `429` + `Retry-After` | Stops floods from a single source and caps botnet damage per project. |
| 5 | Proof of work: HMAC signed, project scoped, expiring challenge; `sha256(challenge:nonce)` must have N leading zero bits; challenge id redeemable once | `400 challenge_*` / `pow_insufficient` | Makes each submission cost CPU time. Adaptive: difficulty grows by one bit per recent submission from the same IP (max +4) and by two bits when the project is above half its hourly cap. Replays are useless. |
| 6 | Exact duplicate (normalised title + body hash, same project, 24 h) | Same IP: silent drop. Other IP: `dup_count += 1` on the original | Repeated spam disappears; the same bug reported by many players becomes a "+N" signal instead of N rows. |
| 7 | Minimum fill time (`EARFUL_MIN_FILL_SECS` since the challenge was issued) | Quarantine (`too_fast`) | Scripts submit instantly; humans need time to type. |
| 8 | Heuristic spam score: link count and density, URL shorteners, link markup, spam phrases, shouting, character runs, repetitive text | Quarantine at `EARFUL_SPAM_THRESHOLD` | Catches the classic link spam that slips through the cheaper layers. |
| 9 | Near duplicate (after embedding): cosine >= 0.95 with another submission from the same IP in the same project within 24 h | Quarantine (`near_duplicate`) | Catches paraphrased or lightly mutated repeats. |

**Quarantine instead of reject.** Soft signals (7, 8, 9) store the item with `status = spam` and a `spam_reason`, visible in the admin Spam tab where it can be restored. False positives are never lost, and the response is identical to a clean submission (`202 {"ok": true}`), so an attacker cannot probe which rule fired. Hard failures that a legit client must react to (validation, rate limit, invalid challenge) return explicit errors.

All rejections and quarantines are counted per UTC day in `block_counts` (one row per day, project and reason, so a flood cannot grow the database) and shown on the dashboard.

## Search

Four modes, selectable per query:

- **Keyword**: each query word becomes a prefix term (`"word"*`), OR'ed, against the stemmed FTS5 index. Ranked by BM25 with column weights title 3, body 1, contact 0.5.
- **Fuzzy**: the query is split into trigrams, which retrieve up to 300 candidates from the trigram index. Each candidate is scored by the mean, over query words, of the best per-word similarity: exact match 1.0, prefix (3+ chars) 0.95, otherwise optimal string alignment distance (adjacent transpositions count as one edit) with a typo budget of 1 edit for 4 to 5 chars, 2 for 6 to 8 and 3 beyond. Whole-phrase matches in the title or body get a bonus. Scores below 0.6 are dropped. Queries made only of 1 to 2 character words fall back to a `LIKE` scan.
- **Semantic**: the query is embedded with the active provider and compared against the in-memory index by cosine similarity (vectors are L2 normalised, so a dot product). Hits are kept when above an absolute floor and within 70 percent of the best hit, capped at 100. The floor depends on the provider because cosine distributions differ: 0.15 for `hash`, 0.25 for Model2Vec (measured on realistic feedback: unrelated pairs median 0.11, p95 0.28; true paraphrases median 0.41), 0.3 for OpenAI compatible models.
- **Model2Vec** ([potion-base-8M](https://huggingface.co/minishlab/potion-base-8M), MIT): a static embedding model distilled from a sentence transformer. Encoding is tokenization plus a mean of per-token vectors from a 30 MB matrix, so it needs no ONNX runtime or GPU: about 13 µs per item and 20 ms to load in a release build, about 80 MB of extra RSS. Tokenization uses the pure Rust `fancy-regex` backend of `tokenizers`, so no C or C++ build. Input is capped at 2000 characters (`MAX_EMBED_CHARS`).
- **Hybrid** (default): reciprocal rank fusion of the three lists, `score = sum(1 / (60 + rank))`. RRF ignores raw score scales, which differ wildly between BM25, edit distance and cosine, and rewards items that several retrievers agree on.

Filters (project, kind, status, date range, labels all/any, unlabeled) scope candidates before ranking and counting, then results are paginated. Without a query, lists are plain newest-first SQL.

The vector index is brute force on purpose. At 10k items with 768 dimensions it is about 30 MB and a full scan takes a few milliseconds; an ANN index would add complexity for no visible gain at this scale.

## Theme labels

Labels and assignments are ordinary SQLite rows. Definition CRUD and atomic add/remove/set
batches share `routes/labels.rs` operations across the admin API and MCP. Assignment
updates preserve status and embeddings, use project-scoped foreign keys, and cascade
when a project, report or label is deleted. Result pages hydrate labels in bounded
batches instead of one query per report. There is no additional worker or model for
labeling: agents classify through MCP when asked, guided by label descriptions and
the `group_feedback_themes` prompt. See [agent workflows](agent-themes.md).

## Background worker

- **Embedding loop**: wakes on new submissions (a `Notify`) or every 5 minutes, embeds up to 32 rows that lack a vector for the current model, stores them, updates the in-memory index, then runs the near duplicate check. Errors back off exponentially from 5 seconds to 10 minutes. Changing the model re-embeds everything automatically.
- **Pruning** (hourly): rate events older than 2 days, expired challenges and sessions, block events older than 90 days.

## Threat model and known limits

In scope: drive-by bots, scripted floods from one or a few IPs, replayed or forged challenges, link spam, copy-paste repeat spam, credential guessing against the admin login, CSRF against the admin.

Known limits, accepted for a personal tool:

- **Botnets** with many IPs and real CPUs can still trickle in submissions. Proof of work and the per-project hourly cap bound the damage; the quarantine view and bulk actions handle the rest. If a project gets targeted, pause it or rotate its key.
- **Ingest keys are public by design.** They identify a project, they do not authenticate a client. Anything shipped to users (game binaries, web pages) can be extracted, so nothing secret goes into them.
- **IP based limits** are coarse: users behind CGNAT or one office share a budget, and IPv6 is limited per /64. Behind a proxy, `EARFUL_TRUSTED_PROXY_HOPS` must match the real proxy chain or every user collapses onto the proxy's address (or, if set too high, clients can spoof their IP).
- **Single admin**, single password, no MFA. Put the admin UI behind an additional gate (VPN, Tailscale, Cloudflare Access) if that matters to you.
- **Embedding providers** see the feedback text. `model2vec` and `hash` run in process and never send data anywhere; for `openai`, prefer a self-hosted Ollama to keep data on your box.
- **Model2Vec quality** is good for short, single language feedback but below transformer models on long or subtle text, and potion-base-8M is English first. Switch to an OpenAI compatible provider if that matters.


## Diagnostic reports

The existing POST /api/v1/feedback and SDK submit accept kind crash, exception and
log alongside bug and feedback. MCP create_feedback accepts the same kinds and
diagnostics. An optional diagnostics JSON object holds at most 65536 serialized
UTF-8 bytes; omitted/null becomes {}. Other types or larger objects return
invalid_diagnostics. The public request limit is 128 KiB; existing body (3-10000
characters), meta (4096 bytes), PoW and rate limits still apply.

Diagnostics round-trip through list/detail, MCP and JSON/CSV exports. Filter by
kind with the existing status/project/label filters. Stats include kind_counts
excluding spam. The daily chart still shows bugs and feedback only. Search indexes
title/body, so include the exception signature there. See docs/crash-reporting.md
in the repository for integration and agent workflows.
