/**
 * Proof of work solver matching `crates/server/src/pow.rs`.
 *
 * Find a decimal `nonce` such that sha256(utf8(`${challenge}:${nonce}`)) starts
 * with at least `bits` zero bits.
 */
import { sha256 } from '@noble/hashes/sha2.js';

const encoder = new TextEncoder();

/** Number of leading zero bits in a byte array. */
export function leadingZeroBits(bytes: Uint8Array): number {
  let bits = 0;
  for (const byte of bytes) {
    if (byte === 0) {
      bits += 8;
      continue;
    }
    return bits + Math.clz32(byte) - 24;
  }
  return bits;
}

/** Work (leading zero bits) a given nonce proves for a challenge. */
export function workBits(challenge: string, nonce: string): number {
  return leadingZeroBits(sha256(encoder.encode(`${challenge}:${nonce}`)));
}

export interface SolveOptions {
  signal?: AbortSignal;
  /** Called on every yield with the number of nonces tried so far. */
  onProgress?: (attempts: number) => void;
  /** Iterations between yields to the event loop. Default 5000. */
  chunk?: number;
}

const yieldToLoop = (): Promise<void> =>
  new Promise((resolve) => {
    if (typeof MessageChannel !== 'undefined') {
      // Faster than setTimeout(0), which browsers clamp to >= 4 ms after nesting.
      const channel = new MessageChannel();
      channel.port1.onmessage = () => {
        channel.port1.close();
        resolve();
      };
      channel.port2.postMessage(null);
    } else {
      setTimeout(resolve, 0);
    }
  });

function abortError(): Error {
  const err = new Error('Proof of work aborted');
  err.name = 'AbortError';
  return err;
}

/**
 * Counts nonces up from 0 and returns the first one with enough work.
 * Hashes the shared prefix once and clones the hasher state per attempt.
 */
export async function solve(challenge: string, bits: number, opts: SolveOptions = {}): Promise<string> {
  const chunk = Math.max(1, opts.chunk ?? 5000);
  const prefix = sha256.create().update(encoder.encode(`${challenge}:`));
  for (let nonce = 0; ; ) {
    if (opts.signal?.aborted) throw abortError();
    const end = nonce + chunk;
    for (; nonce < end; nonce++) {
      const digest = prefix.clone().update(encoder.encode(String(nonce))).digest();
      if (leadingZeroBits(digest) >= bits) return String(nonce);
    }
    opts.onProgress?.(nonce);
    await yieldToLoop();
  }
}
