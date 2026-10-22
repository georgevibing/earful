import { describe, expect, it } from 'vitest';
import { href, parseHash } from './router';

describe('parseHash', () => {
  it('maps paths to routes', () => {
    expect(parseHash('').name).toBe('dashboard');
    expect(parseHash('#/').name).toBe('dashboard');
    expect(parseHash('#/projects').name).toBe('projects');
    expect(parseHash('#/projects/').name).toBe('projects');
    expect(parseHash('#/projects/new').name).toBe('project-new');
    expect(parseHash('#/projects/12')).toMatchObject({ name: 'project', id: 12 });
    expect(parseHash('#/projects/12/edit')).toMatchObject({ name: 'project-edit', id: 12 });
    expect(parseHash('#/feedback').name).toBe('feedback');
    expect(parseHash('#/tokens').name).toBe('tokens');
  });

  it('parses query params', () => {
    const r = parseHash('#/feedback?status=spam&q=crash%20bug');
    expect(r.name).toBe('feedback');
    expect(r.params.get('status')).toBe('spam');
    expect(r.params.get('q')).toBe('crash bug');
  });

  it('rejects unknown and malformed paths', () => {
    expect(parseHash('#/nope').name).toBe('not-found');
    expect(parseHash('#/projects/abc').name).toBe('not-found');
    expect(parseHash('#/projects/0').name).toBe('not-found');
    expect(parseHash('#/projects/1/delete').name).toBe('not-found');
  });

  it('builds hrefs', () => {
    expect(href('/feedback')).toBe('#/feedback');
    expect(href('/feedback', { q: 'a b' })).toBe('#/feedback?q=a+b');
  });
});
