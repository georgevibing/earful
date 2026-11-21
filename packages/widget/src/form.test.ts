import { afterEach, describe, expect, it, vi } from 'vitest';
import { EarfulError, type FeedbackClient, type FeedbackInput } from './client';
import { friendlyError, mount } from './form';

function fakeClient(impl?: (input: FeedbackInput) => Promise<void>) {
  const submitted: FeedbackInput[] = [];
  const client: FeedbackClient & { prepare: ReturnType<typeof vi.fn> } = {
    prepare: vi.fn(async () => {}),
    submit: vi.fn(async (input: FeedbackInput) => {
      submitted.push(input);
      await impl?.(input);
    }),
  };
  return { client, submitted };
}

function setup(opts: Partial<Parameters<typeof mount>[1]> = {}) {
  const host = document.createElement('div');
  document.body.append(host);
  const { client, submitted } = fakeClient();
  const form = mount(host, { key: 'ear_test', project: 'Space Game', client, ...opts });
  const root = form.root;
  const q = <T extends Element>(sel: string) => root.querySelector<T>(sel)!;
  const submit = async () => {
    q<HTMLFormElement>('form').dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(q<HTMLButtonElement>('button[type=submit]').disabled).toBe(false));
  };
  const type = (sel: string, value: string) => {
    const el = q<HTMLInputElement | HTMLTextAreaElement>(sel);
    el.value = value;
    el.dispatchEvent(new Event('input', { bubbles: true }));
  };
  return { host, form, root, q, submit, type, client, submitted };
}

afterEach(() => {
  document.body.replaceChildren();
});

describe('mount', () => {
  it('renders all fields inside a shadow root with the project heading', () => {
    const { host, q } = setup();
    expect(host.shadowRoot).not.toBeNull();
    expect(q('h2').textContent).toBe('Feedback for Space Game');
    expect(q('[data-kind=bug]').textContent).toBe('Bug report');
    expect(q('[data-kind=feedback]').getAttribute('aria-checked')).toBe('true');
    expect(q('input[name=title]')).toBeTruthy();
    expect(q('textarea[name=body]').getAttribute('maxlength')).toBe('10000');
    expect(q('input[name=contact]')).toBeTruthy();
    expect(q('[role=alert]')).toBeTruthy();
    expect(q('[aria-live=polite]')).toBeTruthy();
    // Every visible input has a label.
    for (const id of ['title', 'body', 'contact']) {
      const el = q(`[name=${id}]`);
      expect(q(`label[for="${el.id}"]`)).toBeTruthy();
    }
  });

  it('renders the project title as text, never as markup', () => {
    const { q } = setup({ project: '<img src=x onerror=alert(1)>' });
    expect(q('h2').textContent).toBe('Feedback for <img src=x onerror=alert(1)>');
    expect(q('h2 img')).toBeNull();
  });

  it('includes a hidden honeypot that keyboard users cannot reach', () => {
    const { q } = setup();
    const hp = q<HTMLInputElement>('input[name=website]');
    expect(hp).toBeTruthy();
    expect(hp.getAttribute('tabindex')).toBe('-1');
    expect(hp.getAttribute('autocomplete')).toBe('off');
    const wrap = hp.closest('.earful-hp')!;
    expect(wrap.getAttribute('aria-hidden')).toBe('true');
  });

  it('shows the rating only for feedback', () => {
    const { q } = setup();
    const rating = q<HTMLFieldSetElement>('fieldset.earful-rating');
    expect(rating.hidden).toBe(false);
    q<HTMLButtonElement>('[data-kind=bug]').click();
    expect(rating.hidden).toBe(true);
    expect(q('textarea').getAttribute('placeholder')).toMatch(/Steps to reproduce/);
    q<HTMLButtonElement>('[data-kind=feedback]').click();
    expect(rating.hidden).toBe(false);
  });

  it('updates the live counter', () => {
    const { q, type } = setup();
    type('textarea', 'hello');
    expect(q('.earful-counter').textContent).toBe('5 / 10000');
  });

  it('blocks submission when the description is too short', async () => {
    const { q, submit, type, client } = setup();
    type('textarea', '  a ');
    await submit();
    expect(client.submit).not.toHaveBeenCalled();
    expect(q('[role=alert]').textContent).toMatch(/at least 3 characters/);
    expect(q('textarea').getAttribute('aria-invalid')).toBe('true');
  });

  it('submits trimmed values with metadata and shows the success state', async () => {
    const onSuccess = vi.fn();
    const { q, submit, type, submitted } = setup({ onSuccess, meta: { version: '1.2.3' } });
    type('input[name=title]', '  Great  ');
    type('textarea', '  Love the music  ');
    type('input[name=contact]', 'me@example.com');
    q<HTMLButtonElement>('[data-rating="4"]').click();
    await submit();
    expect(submitted).toHaveLength(1);
    expect(submitted[0]).toMatchObject({
      kind: 'feedback',
      title: 'Great',
      body: 'Love the music',
      contact: 'me@example.com',
      rating: 4,
      website: '',
    });
    expect(submitted[0]!.meta).toMatchObject({ version: '1.2.3', url: expect.any(String), userAgent: expect.any(String) });
    expect(String(submitted[0]!.meta!.url)).not.toMatch(/[?#]/);
    expect(q<HTMLFormElement>('form').hidden).toBe(true);
    expect(q<HTMLElement>('.earful-success').hidden).toBe(false);
    expect(onSuccess).toHaveBeenCalledOnce();

    q<HTMLButtonElement>('.earful-success button').click();
    expect(q<HTMLFormElement>('form').hidden).toBe(false);
    expect(q<HTMLTextAreaElement>('textarea').value).toBe('');
  });

  it('does not send a rating for bug reports', async () => {
    const { q, submit, type, submitted } = setup();
    q<HTMLButtonElement>('[data-rating="2"]').click();
    q<HTMLButtonElement>('[data-kind=bug]').click();
    type('textarea', 'Crashes on start');
    await submit();
    expect(submitted[0]!.kind).toBe('bug');
    expect(submitted[0]!.rating).toBeUndefined();
  });

  it('shows a friendly error and keeps the form when submission fails', async () => {
    const host = document.createElement('div');
    document.body.append(host);
    const { client } = fakeClient(async () => {
      throw new EarfulError('rate_limited', 'slow down', 429, 120);
    });
    const form = mount(host, { key: 'k', client });
    const textarea = form.root.querySelector('textarea')!;
    textarea.value = 'Some feedback';
    form.root.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
    await vi.waitFor(() =>
      expect(form.root.querySelector('[role=alert]')!.textContent).toBe(
        'You are sending feedback too quickly. Please try again in 2 minutes.',
      ),
    );
    expect(form.root.querySelector('form')!.hidden).toBe(false);
  });

  it('starts solving the challenge when the form gets focus', () => {
    const { q, client } = setup();
    q<HTMLInputElement>('input[name=title]').dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    q<HTMLInputElement>('input[name=title]').dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    expect(client.prepare).toHaveBeenCalledTimes(1);
  });

  it('renders a close button when onClose is given', () => {
    const onClose = vi.fn();
    const { q } = setup({ onClose });
    q<HTMLButtonElement>('button[aria-label=Close]').click();
    expect(onClose).toHaveBeenCalledOnce();
  });
});

describe('friendlyError', () => {
  it('maps codes to readable messages', () => {
    expect(friendlyError(new EarfulError('project_paused', '', 403))).toMatch(/not accepting/);
    expect(friendlyError(new EarfulError('network', '', 0))).toMatch(/connection/);
    expect(friendlyError(new EarfulError('pow_insufficient', '', 400))).toMatch(/verify/);
    expect(friendlyError(new EarfulError('invalid_body', 'body too long', 400))).toBe('body too long');
    expect(friendlyError(new EarfulError('rate_limited', '', 429, 1))).toMatch(/1 second\./);
    expect(friendlyError(new Error('boom'))).toMatch(/Something went wrong/);
  });
});
