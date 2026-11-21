import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

afterEach(() => {
  document.body.replaceChildren();
  delete window.Earful;
  vi.resetModules();
});

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 500 })));
});

async function loadWithScript(attrs: Record<string, string>) {
  const script = document.createElement('script');
  script.src = 'https://feedback.example.com/widget.js';
  for (const [k, v] of Object.entries(attrs)) script.setAttribute(k, v);
  // Not appended: happy-dom would try to fetch it. currentScript is all the widget reads.
  Object.defineProperty(document, 'currentScript', { configurable: true, get: () => script });
  await import('./widget');
  return script;
}

describe('widget auto init', () => {
  it('mounts inline forms and exposes the global API', async () => {
    const el = document.createElement('div');
    el.setAttribute('data-earful-inline', '');
    el.setAttribute('data-earful-key', 'ear_inline');
    el.setAttribute('data-earful-project', 'Hosted');
    document.body.append(el);
    await loadWithScript({});

    expect(window.Earful).toBeDefined();
    expect(typeof window.Earful!.createClient).toBe('function');
    expect(typeof window.Earful!.mount).toBe('function');
    expect(el.dataset.earfulMounted).toBe('true');
    expect(el.shadowRoot!.querySelector('h2')!.textContent).toBe('Feedback for Hosted');
    // Running again does not double mount.
    expect(window.Earful!.mountInline()).toHaveLength(0);
  });

  it('still mounts embeds that use the pre-rename data-fh-* attributes', async () => {
    const el = document.createElement('div');
    el.setAttribute('data-fh-inline', '');
    el.setAttribute('data-fh-key', 'fhk_legacy');
    el.setAttribute('data-fh-project', 'Legacy');
    document.body.append(el);
    await loadWithScript({});

    expect(el.dataset.earfulMounted).toBe('true');
    expect(el.shadowRoot!.querySelector('h2')!.textContent).toBe('Feedback for Legacy');
  });

  it('defaults the endpoint to the script origin', async () => {
    const el = document.createElement('div');
    el.setAttribute('data-earful-inline', '');
    el.setAttribute('data-earful-key', 'ear_inline');
    document.body.append(el);
    await loadWithScript({});
    el.shadowRoot!.querySelector('form')!.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    await vi.waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(String(vi.mocked(fetch).mock.calls[0]![0])).toBe(
      'https://feedback.example.com/api/v1/challenge?key=ear_inline',
    );
  });

  it('renders a floating launcher that opens a panel and closes on Escape', async () => {
    await loadWithScript({ 'data-earful-key': 'ear_float', 'data-earful-label': 'Report a bug', 'data-earful-position': 'bottom-left' });
    const host = document.querySelector<HTMLElement>('[data-earful-launcher]')!;
    const root = host.shadowRoot!;
    const button = root.querySelector<HTMLButtonElement>('.earful-launcher')!;
    const panel = root.querySelector<HTMLElement>('.earful-panel')!;
    expect(button.textContent).toBe('Report a bug');
    expect(button.classList.contains('earful-pos-left')).toBe(true);
    expect(panel.hidden).toBe(true);

    button.click();
    expect(panel.hidden).toBe(false);
    expect(button.getAttribute('aria-expanded')).toBe('true');
    expect(panel.querySelector('div')!.shadowRoot!.querySelector('textarea')).toBeTruthy();

    panel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, composed: true }));
    expect(panel.hidden).toBe(true);
    expect(button.getAttribute('aria-expanded')).toBe('false');
  });
});
