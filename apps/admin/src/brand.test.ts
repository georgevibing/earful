import { describe, expect, it } from 'vitest';
import { BRAND, documentTitle, LOGO_URL } from './brand';

describe('brand', () => {
  it('builds per page document titles', () => {
    expect(documentTitle('feedback')).toBe('Inbox · Earful');
    expect(documentTitle('tokens')).toBe('API tokens · Earful');
    expect(documentTitle('login')).toBe('Sign in · Earful');
    expect(BRAND).toBe('Earful');
  });

  it('points the logo at the public folder', () => {
    expect(LOGO_URL.endsWith('logo.svg')).toBe(true);
  });
});
