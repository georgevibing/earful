# packages/widget

Browser client, proof of work solver and the embeddable `widget.js`. The server embeds `dist/widget.js` and serves it at `/widget.js`; the hosted page `/s/<slug>` uses it in inline mode.

## Commands

- `pnpm --filter @feedback-harvest/widget build` (ESM client, IIFE `dist/widget.js`, type declarations)
- `pnpm --filter @feedback-harvest/widget test` (vitest)
- `pnpm --filter @feedback-harvest/widget typecheck`

## Gotchas

- `src/pow.ts` must stay byte-for-byte compatible with `crates/server/src/pow.rs`: `sha256(utf8(challenge + ":" + nonce))`, decimal nonce, leading zero bits. The shared vector lives in `docs/api.md` and is asserted in both test suites.
- The widget runs on third-party pages: render inside Shadow DOM, no globals besides the documented ones, no `innerHTML` with any dynamic data.
- No external network requests except to the configured feedback-harvest endpoint. No CDNs.
- Keep `dist/widget.js` small (it loads on customer pages). `@noble/hashes` is the only runtime dependency.
- Fetch the challenge when the form opens so solving and the server's minimum fill time overlap with typing.
- After a server rebuild in release mode, the widget is re-embedded only if `dist/` was rebuilt first.
