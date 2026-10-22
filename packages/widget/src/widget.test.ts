import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

afterEach(() => {
  document.body.replaceChildren();
  delete window.FeedbackHarvest;
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
    el.setAttribute('data-fh-inline', '');
    el.setAttribute('data-fh-key', 'fhk_inline');
    el.setAttribute('data-fh-project', 'Hosted');
    document.body.append(el);
    await loadWithScript({});

    expect(window.FeedbackHarvest).toBeDefined();
    expect(typeof window.FeedbackHarvest!.createClient).toBe('function');
    expect(typeof window.FeedbackHarvest!.mount).toBe('function');
    expect(el.dataset.fhMounted).toBe('true');
    expect(el.shadowRoot!.querySelector('h2')!.textContent).toBe('Feedback for Hosted');
    // Running again does not double mount.
    expect(window.FeedbackHarvest!.mountInline()).toHaveLength(0);
  });

  it('defaults the endpoint to the script origin', async () => {
    const el = document.createElement('div');
    el.setAttribute('data-fh-inline', '');
    el.setAttribute('data-fh-key', 'fhk_inline');
    document.body.append(el);
    await loadWithScript({});
    el.shadowRoot!.querySelector('form')!.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    await vi.waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(String(vi.mocked(fetch).mock.calls[0]![0])).toBe(
      'https://feedback.example.com/api/v1/challenge?key=fhk_inline',
    );
  });

  it('renders a floating launcher that opens a panel and closes on Escape', async () => {
    await loadWithScript({ 'data-fh-key': 'fhk_float', 'data-fh-label': 'Report a bug', 'data-fh-position': 'bottom-left' });
    const host = document.querySelector<HTMLElement>('[data-fh-launcher]')!;
    const root = host.shadowRoot!;
    const button = root.querySelector<HTMLButtonElement>('.fh-launcher')!;
    const panel = root.querySelector<HTMLElement>('.fh-panel')!;
    expect(button.textContent).toBe('Report a bug');
    expect(button.classList.contains('fh-pos-left')).toBe(true);
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
