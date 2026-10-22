import { describe, expect, it } from 'vitest';
import { compact, highlight, niceMax, relativeTime, searchTerms, slugify, snippet, validSlug } from './util';

describe('slugify (mirrors models::slugify)', () => {
  it('matches the server cases', () => {
    expect(slugify('My Cool Game!')).toBe('my-cool-game');
    expect(slugify('  --Ünïcode  Thing--')).toBe('n-code-thing');
    expect(slugify('')).toBe('');
    expect(slugify('a'.repeat(80))).toHaveLength(63);
  });
  it('validates', () => {
    expect(validSlug('abc-123')).toBe(true);
    expect(validSlug('-abc')).toBe(false);
    expect(validSlug('abc-')).toBe(false);
    expect(validSlug('ABC')).toBe(false);
    expect(validSlug('')).toBe(false);
  });
});

describe('relativeTime', () => {
  const now = Date.UTC(2026, 9, 22, 12, 0, 0);
  const ts = now / 1000;
  it('formats recent times', () => {
    expect(relativeTime(ts - 10, now)).toBe('just now');
    expect(relativeTime(ts - 5 * 60, now)).toBe('5m ago');
    expect(relativeTime(ts - 3 * 3600, now)).toBe('3h ago');
    expect(relativeTime(ts - 2 * 86400, now)).toBe('2d ago');
    expect(relativeTime(ts - 40 * 86400, now)).toMatch(/^2026-09-1\d$/);
  });
});

describe('highlight', () => {
  it('splits case-insensitively and keeps original casing', () => {
    expect(highlight('Game CRASHES on boot', ['crash'])).toEqual([
      { text: 'Game ', match: false },
      { text: 'CRASH', match: true },
      { text: 'ES on boot', match: false },
    ]);
  });
  it('handles no terms, empty text and adjacent matches', () => {
    expect(highlight('abc', [])).toEqual([{ text: 'abc', match: false }]);
    expect(highlight('', ['a'])).toEqual([]);
    expect(highlight('abab', ['ab'])).toEqual([
      { text: 'ab', match: true },
      { text: 'ab', match: true },
    ]);
  });
  it('prefers longer terms', () => {
    expect(searchTerms('in inventory x')).toEqual(['inventory', 'in']);
    expect(highlight('inventory', searchTerms('in inventory'))).toEqual([{ text: 'inventory', match: true }]);
  });
});

describe('snippet', () => {
  it('centres on the first match', () => {
    const body = `${'lorem '.repeat(60)}the boss fight crashed ${'ipsum '.repeat(60)}`;
    const s = snippet(body, ['crashed'], 80);
    expect(s).toContain('crashed');
    expect(s.startsWith('...')).toBe(true);
    expect(s.endsWith('...')).toBe(true);
  });
  it('returns short bodies untouched, collapsed to one line', () => {
    expect(snippet('a\n\nb', [])).toBe('a b');
  });
});

describe('numbers', () => {
  it('compacts', () => {
    expect(compact(1284)).toBe('1,284');
    expect(compact(12900)).toBe('12.9K');
    expect(compact(4_200_000)).toBe('4.2M');
    expect(compact(20000)).toBe('20K');
  });
  it('finds nice axis maxima', () => {
    expect(niceMax(0)).toBe(1);
    expect(niceMax(3)).toBe(5);
    expect(niceMax(7)).toBe(10);
    expect(niceMax(13)).toBe(20);
    expect(niceMax(100)).toBe(100);
  });
});

import { showTick } from './components/DailyChart';

describe('chart ticks', () => {
  it('labels weeks and the last day without collisions', () => {
    const ticks = Array.from({ length: 30 }, (_, i) => i).filter((i) => showTick(i, 30));
    expect(ticks).toEqual([0, 7, 14, 21, 29]);
    const ticks27 = Array.from({ length: 30 }, (_, i) => i).filter((i) => showTick(i, 29));
    expect(ticks27).toEqual([0, 7, 14, 21, 28]);
    expect(Array.from({ length: 30 }, (_, i) => i).filter((i) => showTick(i, 31))).toEqual([0, 7, 14, 21]);
  });
});
