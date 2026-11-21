/**
 * Minimal client for the earful ingestion API (see docs/api.md).
 * Handles challenge fetching, proof of work and one retry on stale challenges.
 */
import { solve } from './pow';

/** Highest proof of work difficulty the widget will attempt (server default max is 23). */
export const MAX_BITS = 28;
const MAX_SOLVE_MS = 120_000;

function isSaneBits(bits: unknown): bits is number {
  return typeof bits === 'number' && Number.isInteger(bits) && bits >= 0 && bits <= MAX_BITS;
}

export type FeedbackKind = 'bug' | 'feedback' | 'crash' | 'exception' | 'log';

export interface FeedbackInput {
  kind: FeedbackKind;
  title?: string;
  body: string;
  contact?: string;
  /** 1 to 5, only meaningful for `feedback`. */
  rating?: number;
  /** Bounded diagnostic context, at most 64 KiB serialized. */
  diagnostics?: Record<string, unknown>;
  meta?: Record<string, unknown>;
  /** Honeypot. Leave empty; bots that fill every field get silently dropped. */
  website?: string;
}

export interface ClientOptions {
  /** Base URL of the earful server, e.g. `https://feedback.example.com`. */
  endpoint: string;
  /** The project's public ingest key (`ear_...`). */
  key: string;
  fetch?: typeof fetch;
  /** Clock override for tests. */
  now?: () => number;
}

export interface FeedbackClient {
  /** Fetches and solves a challenge in the background. Safe to call repeatedly. */
  prepare(): Promise<void>;
  /** Submits feedback, solving a challenge first if none is ready. */
  submit(input: FeedbackInput): Promise<void>;
}

export class EarfulError extends Error {
  override readonly name = 'EarfulError';
  constructor(
    /** Stable server error code, or `network` / `invalid_response` / `challenge_timeout` for client side failures. */
    readonly code: string,
    message: string,
    /** HTTP status, 0 for network failures. */
    readonly status: number,
    /** Seconds to wait before retrying, from the `Retry-After` header. */
    readonly retryAfter?: number,
  ) {
    super(message);
  }
}

interface ChallengeResponse {
  challenge: string;
  bits: number;
  expires_in: number;
}

interface Solution {
  challenge: string;
  nonce: string;
  validUntil: number;
}

/** Discard prepared challenges this long before the server would expire them. */
const EXPIRY_MARGIN_MS = 30_000;
const RETRYABLE = new Set(['challenge_used', 'challenge_expired']);

async function toError(res: Response): Promise<EarfulError> {
  let code = 'http_error';
  let message = `Request failed with status ${res.status}`;
  try {
    const data: unknown = await res.json();
    if (data && typeof data === 'object') {
      const d = data as { error?: unknown; message?: unknown };
      if (typeof d.error === 'string') code = d.error;
      if (typeof d.message === 'string') message = d.message;
    }
  } catch {
    // Non JSON error body, keep the generic message.
  }
  const header = res.headers.get('retry-after');
  const retryAfter = header !== null && /^\d+$/.test(header.trim()) ? Number(header) : undefined;
  return new EarfulError(code, message, res.status, retryAfter);
}

export function createClient(options: ClientOptions): FeedbackClient {
  const endpoint = options.endpoint.replace(/\/+$/, '');
  const doFetch: typeof fetch = options.fetch ?? ((input, init) => globalThis.fetch(input, init));
  const now = options.now ?? Date.now;
  let pending: Promise<Solution> | null = null;

  async function request(url: string, init?: RequestInit): Promise<Response> {
    try {
      return await doFetch(url, init);
    } catch (err) {
      throw new EarfulError('network', err instanceof Error ? err.message : 'Network error', 0);
    }
  }

  async function fetchSolution(): Promise<Solution> {
    const res = await request(`${endpoint}/api/v1/challenge?key=${encodeURIComponent(options.key)}`, {
      method: 'GET',
      credentials: 'omit',
      cache: 'no-store',
    });
    if (!res.ok) throw await toError(res);
    const data = (await res.json()) as Partial<ChallengeResponse>;
    if (typeof data.challenge !== 'string' || !isSaneBits(data.bits)) {
      throw new EarfulError('invalid_response', 'Malformed challenge response', res.status);
    }
    const issuedAt = now();
    // A hostile endpoint must not be able to keep the visitor's CPU busy forever.
    const timeout = new AbortController();
    const timer = setTimeout(() => timeout.abort(), MAX_SOLVE_MS);
    let nonce: string;
    try {
      nonce = await solve(data.challenge, data.bits, { signal: timeout.signal });
    } catch (err) {
      if (timeout.signal.aborted) {
        throw new EarfulError('challenge_timeout', 'Solving the challenge took too long', 0);
      }
      throw err;
    } finally {
      clearTimeout(timer);
    }
    const ttlMs = (typeof data.expires_in === 'number' ? data.expires_in : 0) * 1000;
    return { challenge: data.challenge, nonce, validUntil: issuedAt + ttlMs - EXPIRY_MARGIN_MS };
  }

  function ensurePending(): Promise<Solution> {
    if (!pending) {
      const p = fetchSolution();
      pending = p;
      // Forget failed attempts so the next call starts fresh.
      p.catch(() => {
        if (pending === p) pending = null;
      });
    }
    return pending;
  }

  /** Takes a fresh solution, discarding one that expired while it sat around. */
  async function take(): Promise<Solution> {
    let solution = await ensurePending();
    pending = null;
    if (now() >= solution.validUntil) {
      solution = await ensurePending();
      pending = null;
    }
    return solution;
  }

  return {
    async prepare() {
      await ensurePending();
    },

    async submit(input) {
      for (let attempt = 0; ; attempt++) {
        const { challenge, nonce } = await take();
        const res = await request(`${endpoint}/api/v1/feedback`, {
          method: 'POST',
          credentials: 'omit',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            key: options.key,
            kind: input.kind,
            title: input.title ?? '',
            body: input.body,
            contact: input.contact || undefined,
            rating: input.kind === 'feedback' ? input.rating : undefined,
            meta: input.meta,
            website: input.website ?? '',
            source: 'web',
            challenge,
            nonce,
          }),
        });
        if (res.ok) return;
        const err = await toError(res);
        if (attempt === 0 && RETRYABLE.has(err.code)) continue;
        throw err;
      }
    },
  };
}
