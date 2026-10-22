/** Mirrors `models::slugify` on the server. */
export function slugify(input: string): string {
  let out = '';
  for (const c of input.toLowerCase()) {
    if (/^[a-z0-9]$/.test(c)) out += c;
    else if (out !== '' && !out.endsWith('-')) out += '-';
  }
  return out.replace(/-+$/, '').slice(0, 63).replace(/-+$/, '');
}

export function validSlug(slug: string): boolean {
  return /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(slug);
}

/** Human friendly relative time for a Unix seconds timestamp. */
export function relativeTime(ts: number, nowMs: number = Date.now()): string {
  const diff = Math.round(nowMs / 1000 - ts);
  if (diff < 45) return 'just now';
  if (diff < 3600) return `${Math.round(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.round(diff / 3600)}h ago`;
  if (diff < 86400 * 14) return `${Math.round(diff / 86400)}d ago`;
  return formatDate(ts);
}

export function formatDate(ts: number): string {
  const d = new Date(ts * 1000);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function formatDateTime(ts: number): string {
  const d = new Date(ts * 1000);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${formatDate(ts)} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 1284 -> "1,284", 12900 -> "12.9K", 4200000 -> "4.2M". */
export function compact(n: number): string {
  const abs = Math.abs(n);
  if (abs < 10000) return n.toLocaleString('en-US');
  if (abs < 1_000_000) return `${trim1(n / 1000)}K`;
  return `${trim1(n / 1_000_000)}M`;
}

function trim1(n: number): string {
  return n.toFixed(1).replace(/\.0$/, '');
}

/** Lowercased alphanumeric search terms (length >= 2), deduplicated, longest first. */
export function searchTerms(q: string): string[] {
  const terms = q
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter((t) => t.length >= 2);
  return [...new Set(terms)].sort((a, b) => b.length - a.length);
}

export interface Segment {
  text: string;
  match: boolean;
}

/** Splits text into plain and matching segments for case-insensitive highlighting. */
export function highlight(text: string, terms: string[]): Segment[] {
  if (!text) return [];
  if (terms.length === 0) return [{ text, match: false }];
  const lower = text.toLowerCase();
  const segments: Segment[] = [];
  let i = 0;
  let plainStart = 0;
  while (i < text.length) {
    const term = terms.find((t) => lower.startsWith(t, i));
    if (term) {
      if (plainStart < i) segments.push({ text: text.slice(plainStart, i), match: false });
      segments.push({ text: text.slice(i, i + term.length), match: true });
      i += term.length;
      plainStart = i;
    } else {
      i += 1;
    }
  }
  if (plainStart < text.length) segments.push({ text: text.slice(plainStart), match: false });
  return segments;
}

/** A window of `body` around the first term match (or its start), single line. */
export function snippet(body: string, terms: string[], width = 180): string {
  const flat = body.replace(/\s+/g, ' ').trim();
  if (flat.length <= width) return flat;
  const lower = flat.toLowerCase();
  let pos = -1;
  for (const t of terms) {
    const p = lower.indexOf(t);
    if (p >= 0 && (pos < 0 || p < pos)) pos = p;
  }
  if (pos < 0) return `${flat.slice(0, width).trimEnd()}...`;
  const start = Math.max(0, pos - Math.floor(width / 3));
  const end = Math.min(flat.length, start + width);
  return `${start > 0 ? '...' : ''}${flat.slice(start, end).trim()}${end < flat.length ? '...' : ''}`;
}

/** Smallest "nice" axis maximum >= n (1, 2, 5 x 10^k). */
export function niceMax(n: number): number {
  if (n <= 0) return 1;
  const pow = 10 ** Math.floor(Math.log10(n));
  for (const m of [1, 2, 5, 10]) {
    if (m * pow >= n) return m * pow;
  }
  return 10 * pow;
}

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
