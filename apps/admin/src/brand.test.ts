import { describe, expect, it } from 'vitest';
import { BRAND, documentTitle, LOGO_URL } from './brand';

describe('brand', () => {
  it('builds per page document titles', () => {
    expect(documentTitle('feedback')).toBe('Inbox · Feedback Harvest');
    expect(documentTitle('tokens')).toBe('API tokens · Feedback Harvest');
    expect(documentTitle('login')).toBe('Sign in · Feedback Harvest');
    expect(BRAND).toBe('Feedback Harvest');
  });

  it('points the logo at the public folder', () => {
    expect(LOGO_URL.endsWith('logo.svg')).toBe(true);
  });
});
