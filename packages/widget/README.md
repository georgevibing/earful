# @feedback-harvest/widget

Browser side of feedback-harvest. It ships two builds:

- `dist/widget.js`: a self contained IIFE (about 22 KB raw, 9 KB gzip) that the server embeds and serves at `/widget.js`. Drop it on a page and it renders a feedback form.
- `dist/client.js`: a small ESM client (proof of work solver and submit logic) for when you want your own UI.

Everything renders inside a Shadow DOM, so your page styles do not leak in and the widget styles do not leak out. It follows `prefers-color-scheme` for light and dark mode.

## Floating button

Add the script tag with your project's ingest key. A button appears in the corner and opens a small panel.

```html
<script
  src="https://feedback.example.com/widget.js"
  data-fh-key="fhk_your_ingest_key"
  data-fh-label="Feedback"
  data-fh-position="bottom-right"
  defer
></script>
```

| Attribute | Default | Meaning |
| --- | --- | --- |
| `data-fh-key` | required | Project ingest key (public by design). |
| `data-fh-label` | `Feedback` | Button text. |
| `data-fh-position` | `bottom-right` | `bottom-right` or `bottom-left`. |
| `data-fh-endpoint` | origin of the script `src` | Server base URL, if the script is served from somewhere else. |
| `data-fh-project` | none | Project name shown in the heading. |
| `data-fh-kind` | `feedback` | Initially selected type, `bug` or `feedback`. |
| `data-fh-meta` | none | JSON object merged into the submitted `meta`, e.g. `{"version":"1.4.2"}`. |

Escape closes the panel and returns focus to the button.

## Inline form

Put a container anywhere on the page. Every `[data-fh-inline]` element gets its own form.

```html
<div data-fh-inline data-fh-key="fhk_your_ingest_key" data-fh-project="My Game"></div>
<script src="https://feedback.example.com/widget.js" defer></script>
```

The inline container accepts the same `data-fh-*` attributes as above (except label and position). If the container has no `data-fh-key`, the one on the script tag is used.

## Hosted page

Every project also has a hosted page at `https://feedback.example.com/s/<project-slug>`. It is just the inline form, so you can link to it from a game menu, a Steam page, a README or an email, with no embedding at all.

## JavaScript API

The bundle exposes `window.FeedbackHarvest`:

```js
// Mount a form into any element (for example after a SPA route change).
FeedbackHarvest.mount(document.querySelector('#feedback'), {
  key: 'fhk_your_ingest_key',
  endpoint: 'https://feedback.example.com',
  project: 'My Web App',
  meta: { version: '2.0.1', userTier: 'free' },
  onSuccess: () => console.log('sent'),
});

// Re-scan for new [data-fh-inline] elements.
FeedbackHarvest.mountInline();
```

## ESM client (bring your own UI)

```ts
import { createClient, FeedbackHarvestError } from '@feedback-harvest/widget';

const client = createClient({ endpoint: 'https://feedback.example.com', key: 'fhk_your_ingest_key' });

// Optional: start the proof of work early, e.g. when your dialog opens.
void client.prepare();

try {
  await client.submit({
    kind: 'bug',
    title: 'Crash on save',
    body: 'Game freezes after the autosave icon appears.',
    contact: 'player@example.com',
    meta: { version: '1.4.2', platform: 'linux' },
  });
} catch (err) {
  if (err instanceof FeedbackHarvestError && err.code === 'rate_limited') {
    console.log(`retry in ${err.retryAfter}s`);
  }
}
```

`submit` fetches and solves a challenge if none is prepared, discards prepared challenges that are about to expire, and retries once with a fresh challenge when the server answers `challenge_used` or `challenge_expired`. Errors are `FeedbackHarvestError` with `code` (the server's stable error code, or `network` / `invalid_response`), `status` and, for rate limits, `retryAfter` in seconds.

Also exported: `solve`, `workBits`, `leadingZeroBits`, `mount`, `collectMeta`, `friendlyError`.

## What gets collected

Only what the user types plus this `meta`, merged with anything you pass:

```json
{ "url": "https://example.com/page", "userAgent": "...", "language": "en-US", "screen": "1920x1080", "viewport": "1280x720" }
```

The URL has its query string and hash stripped. A hidden honeypot field named `website` is included; humans never see or reach it.

## Protocol for non JS clients (games, native apps, scripts)

You do not need this package to submit feedback. The full contract is in [`docs/api.md`](../../docs/api.md); in short:

1. `GET /api/v1/challenge?key=<ingest_key>` returns `{ "challenge", "bits", "expires_in" }`. Fetch it when the feedback dialog opens.
2. Find a decimal `nonce` (count up from 0) such that `sha256(utf8(challenge + ":" + nonce))` starts with at least `bits` zero bits. Around 18 bits by default, a fraction of a second in any language.
3. `POST /api/v1/feedback` with JSON `{ key, kind: "bug" | "feedback" | "crash" | "exception" | "log", title, body, contact, rating, meta, challenge, nonce }`. Success is always `202 {"ok": true}`.

Each challenge works once and expires after `expires_in` seconds. On `challenge_used` or `challenge_expired`, fetch a new one and retry.

## Development

```sh
pnpm --filter @feedback-harvest/widget build      # dist/client.js, dist/widget.js, dist/types
pnpm --filter @feedback-harvest/widget test       # vitest (happy-dom)
pnpm --filter @feedback-harvest/widget typecheck
```

The PoW test vector in `src/pow.test.ts` is shared with `crates/server/src/pow.rs`. Change both together.


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
