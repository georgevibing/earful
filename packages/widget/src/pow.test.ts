import { describe, expect, it } from 'vitest';
import { leadingZeroBits, solve, workBits } from './pow';

// Shared with crates/server/src/pow.rs::cross_language_vector. Keep both in sync.
const VECTOR = 'v1.1.1700000000000.12.0123456789abcdef0123456789abcdef.deadbeef';

describe('leadingZeroBits', () => {
  it('counts bits across bytes', () => {
    expect(leadingZeroBits(new Uint8Array([0xff]))).toBe(0);
    expect(leadingZeroBits(new Uint8Array([0x00, 0x80]))).toBe(8);
    expect(leadingZeroBits(new Uint8Array([0x00, 0x0f]))).toBe(12);
    expect(leadingZeroBits(new Uint8Array([0x00, 0x00]))).toBe(16);
    expect(leadingZeroBits(new Uint8Array([0x01]))).toBe(7);
  });
});

describe('solve', () => {
  it('matches the Rust reference solver on the shared vector', async () => {
    const nonce = await solve(VECTOR, 12);
    expect(nonce).toBe('1627');
    expect(workBits(VECTOR, nonce)).toBeGreaterThanOrEqual(12);
  });

  it('agrees with workBits for every nonce below the answer', async () => {
    const nonce = await solve(VECTOR, 12, { chunk: 100 });
    for (let n = 0; n < Number(nonce); n++) {
      expect(workBits(VECTOR, String(n))).toBeLessThan(12);
    }
  });

  it('reports progress and can be aborted', async () => {
    const controller = new AbortController();
    const seen: number[] = [];
    const run = solve('impossible', 64, {
      chunk: 50,
      signal: controller.signal,
      onProgress: (n) => {
        seen.push(n);
        if (seen.length === 3) controller.abort();
      },
    });
    await expect(run).rejects.toMatchObject({ name: 'AbortError' });
    expect(seen).toEqual([50, 100, 150]);
  });

  it('returns 0 when no work is required', async () => {
    expect(await solve('anything', 0)).toBe('0');
  });
});
