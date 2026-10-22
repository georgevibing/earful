import { describe, expect, it, vi } from 'vitest';
import { createClient, FeedbackHarvestError } from './client';
import { workBits } from './pow';

const json = (status: number, body: unknown, headers: Record<string, string> = {}) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json', ...headers } });

let counter = 0;
const challenge = () => ({ challenge: `v1.1.${Date.now()}.4.${String(++counter).padStart(32, '0')}.sig`, bits: 4, expires_in: 1800 });

type Call = { url: string; init: RequestInit | undefined };

function mockServer(responses: Array<(call: Call) => Response>) {
  const calls: Call[] = [];
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const call = { url: String(input), init };
    calls.push(call);
    const next = responses.shift();
    if (!next) throw new Error(`unexpected request ${call.url}`);
    return next(call);
  });
  return { calls, fetch: fetchMock as unknown as typeof fetch };
}

describe('createClient', () => {
  it('fetches a challenge, solves it and posts the expected body', async () => {
    const c = challenge();
    const server = mockServer([() => json(200, c), () => json(202, { ok: true })]);
    const client = createClient({ endpoint: 'https://fb.example.com/', key: 'fhk_abc', fetch: server.fetch });

    await client.submit({ kind: 'feedback', title: 'Hi', body: 'Great game', rating: 5, meta: { v: '1' } });

    expect(server.calls[0]!.url).toBe('https://fb.example.com/api/v1/challenge?key=fhk_abc');
    expect(server.calls[0]!.init?.credentials).toBe('omit');
    const post = server.calls[1]!;
    expect(post.url).toBe('https://fb.example.com/api/v1/feedback');
    expect(post.init?.method).toBe('POST');
    expect(new Headers(post.init?.headers).get('content-type')).toBe('application/json');
    const body = JSON.parse(String(post.init?.body));
    expect(body).toMatchObject({
      key: 'fhk_abc',
      kind: 'feedback',
      title: 'Hi',
      body: 'Great game',
      rating: 5,
      meta: { v: '1' },
      website: '',
      source: 'web',
      challenge: c.challenge,
    });
    expect(workBits(body.challenge, body.nonce)).toBeGreaterThanOrEqual(4);
  });

  it('drops rating for bug reports and omits empty contact', async () => {
    const server = mockServer([() => json(200, challenge()), () => json(202, { ok: true })]);
    const client = createClient({ endpoint: 'https://x', key: 'k', fetch: server.fetch });
    await client.submit({ kind: 'bug', body: 'It crashes', rating: 3, contact: '' });
    const body = JSON.parse(String(server.calls[1]!.init?.body));
    expect(body.rating).toBeUndefined();
    expect(body.contact).toBeUndefined();
  });

  it('reuses a prepared challenge instead of fetching another', async () => {
    const server = mockServer([() => json(200, challenge()), () => json(202, { ok: true })]);
    const client = createClient({ endpoint: 'https://x', key: 'k', fetch: server.fetch });
    await client.prepare();
    await client.prepare();
    await client.submit({ kind: 'bug', body: 'abc' });
    expect(server.calls).toHaveLength(2);
  });

  it('retries once with a fresh challenge when the server reports challenge_used', async () => {
    const first = challenge();
    const second = challenge();
    const server = mockServer([
      () => json(200, first),
      () => json(400, { error: 'challenge_used', message: 'used' }),
      () => json(200, second),
      () => json(202, { ok: true }),
    ]);
    const client = createClient({ endpoint: 'https://x', key: 'k', fetch: server.fetch });
    await client.submit({ kind: 'bug', body: 'abc' });
    expect(server.calls).toHaveLength(4);
    expect(JSON.parse(String(server.calls[3]!.init?.body)).challenge).toBe(second.challenge);
  });

  it('gives up after one retry', async () => {
    const server = mockServer([
      () => json(200, challenge()),
      () => json(400, { error: 'challenge_expired', message: 'expired' }),
      () => json(200, challenge()),
      () => json(400, { error: 'challenge_expired', message: 'expired' }),
    ]);
    const client = createClient({ endpoint: 'https://x', key: 'k', fetch: server.fetch });
    await expect(client.submit({ kind: 'bug', body: 'abc' })).rejects.toMatchObject({ code: 'challenge_expired' });
  });

  it('refreshes a prepared challenge that expired locally', async () => {
    let clock = 1_000_000;
    const server = mockServer([
      () => json(200, { ...challenge(), expires_in: 60 }),
      () => json(200, challenge()),
      () => json(202, { ok: true }),
    ]);
    const client = createClient({ endpoint: 'https://x', key: 'k', fetch: server.fetch, now: () => clock });
    await client.prepare();
    clock += 45_000; // inside the 30s safety margin of a 60s challenge
    await client.submit({ kind: 'bug', body: 'abc' });
    expect(server.calls.filter((c) => c.url.includes('/challenge'))).toHaveLength(2);
  });

  it('maps rate limits to a typed error with retryAfter', async () => {
    const server = mockServer([
      () => json(429, { error: 'rate_limited', message: 'slow down' }, { 'Retry-After': '42' }),
    ]);
    const client = createClient({ endpoint: 'https://x', key: 'k', fetch: server.fetch });
    const err = await client.submit({ kind: 'bug', body: 'abc' }).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(FeedbackHarvestError);
    expect(err).toMatchObject({ code: 'rate_limited', status: 429, retryAfter: 42, message: 'slow down' });
  });

  it('maps network failures and recovers on the next call', async () => {
    const server = mockServer([
      () => {
        throw new TypeError('Failed to fetch');
      },
      () => json(200, challenge()),
      () => json(202, { ok: true }),
    ]);
    const client = createClient({ endpoint: 'https://x', key: 'k', fetch: server.fetch });
    await expect(client.submit({ kind: 'bug', body: 'abc' })).rejects.toMatchObject({ code: 'network', status: 0 });
    await expect(client.submit({ kind: 'bug', body: 'abc' })).resolves.toBeUndefined();
  });

  it('handles non JSON error bodies', async () => {
    const server = mockServer([() => new Response('bad gateway', { status: 502 })]);
    const client = createClient({ endpoint: 'https://x', key: 'k', fetch: server.fetch });
    await expect(client.submit({ kind: 'bug', body: 'abc' })).rejects.toMatchObject({ code: 'http_error', status: 502 });
  });
});
