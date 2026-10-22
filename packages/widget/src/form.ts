/**
 * Feedback form rendered into a Shadow DOM. Built with DOM APIs only; user and
 * server provided strings are always assigned via textContent or attributes.
 */
import { createClient, FeedbackHarvestError, type FeedbackClient, type FeedbackKind } from './client';
import { applyStyles } from './styles';

export const BODY_MIN = 3;
export const BODY_MAX = 10_000;
export const TITLE_MAX = 200;
export const CONTACT_MAX = 200;

export interface MountOptions {
  /** Project ingest key. */
  key: string;
  /** Server base URL. Defaults to the current page origin. */
  endpoint?: string;
  /** Project title shown in the heading. */
  project?: string;
  /** Initially selected kind. Default `feedback`. */
  kind?: FeedbackKind;
  /** Extra metadata merged over the automatically collected fields. */
  meta?: Record<string, unknown>;
  /** Renders a close button that calls this. */
  onClose?: () => void;
  onSuccess?: () => void;
  /** Use an existing client (mostly for tests). */
  client?: FeedbackClient;
  fetch?: typeof fetch;
}

export interface MountedForm {
  /** Starts fetching and solving a challenge now. */
  prepare(): void;
  /** Moves focus to the first field. */
  focus(): void;
  /** Resets the form to its initial empty state. */
  reset(): void;
  destroy(): void;
  readonly root: ShadowRoot;
}

type Attrs = Record<string, string | boolean | undefined>;

function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  children: Array<Node | string> = [],
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [name, value] of Object.entries(attrs)) {
    if (value === undefined || value === false) continue;
    el.setAttribute(name, value === true ? '' : value);
  }
  for (const child of children) el.append(child);
  return el;
}

let idCounter = 0;
const uid = (name: string) => `fh-${name}-${++idCounter}`;

/** Automatically collected context. Query string and hash are stripped from the URL. */
export function collectMeta(): Record<string, string> {
  const meta: Record<string, string> = {};
  if (typeof location !== 'undefined') meta.url = `${location.origin}${location.pathname}`;
  if (typeof navigator !== 'undefined') {
    meta.userAgent = navigator.userAgent;
    meta.language = navigator.language;
  }
  if (typeof screen !== 'undefined') meta.screen = `${screen.width}x${screen.height}`;
  if (typeof window !== 'undefined') meta.viewport = `${window.innerWidth}x${window.innerHeight}`;
  return meta;
}

function formatWait(seconds: number): string {
  if (seconds < 60) return `${seconds} second${seconds === 1 ? '' : 's'}`;
  const minutes = Math.ceil(seconds / 60);
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? '' : 's'}`;
  const hours = Math.ceil(minutes / 60);
  return `${hours} hour${hours === 1 ? '' : 's'}`;
}

/** Turns an API error into something a player or visitor can act on. */
export function friendlyError(err: unknown): string {
  if (!(err instanceof FeedbackHarvestError)) return 'Something went wrong. Please try again.';
  switch (err.code) {
    case 'rate_limited':
      return err.retryAfter
        ? `You are sending feedback too quickly. Please try again in ${formatWait(err.retryAfter)}.`
        : 'You are sending feedback too quickly. Please try again later.';
    case 'project_paused':
      return 'This project is not accepting feedback right now.';
    case 'not_found':
      return 'This feedback form is not set up correctly.';
    case 'network':
      return 'Could not reach the feedback server. Check your connection and try again.';
    case 'challenge_malformed':
    case 'challenge_invalid':
    case 'challenge_wrong_project':
    case 'challenge_expired':
    case 'challenge_used':
    case 'pow_insufficient':
      return 'We could not verify your submission. Please try again.';
    default:
      if (err.code.startsWith('invalid_') && err.message) return err.message;
      return 'Something went wrong. Please try again.';
  }
}

const PLACEHOLDERS: Record<FeedbackKind, string> = {
  bug: 'What happened? What did you expect? Steps to reproduce help a lot.',
  feedback: 'What do you like? What would you change?',
};

export function mount(host: HTMLElement, options: MountOptions): MountedForm {
  const root = host.shadowRoot ?? host.attachShadow({ mode: 'open' });
  applyStyles(root);

  const client =
    options.client ??
    createClient({
      endpoint: options.endpoint ?? (typeof location !== 'undefined' ? location.origin : ''),
      key: options.key,
      ...(options.fetch ? { fetch: options.fetch } : {}),
    });

  let kind: FeedbackKind = options.kind ?? 'feedback';
  let rating: number | undefined;
  let prepared = false;

  const ids = { heading: uid('heading'), title: uid('title'), body: uid('body'), counter: uid('counter'), contact: uid('contact'), hp: uid('hp') };

  const heading = h('h2', { class: 'fh-heading', id: ids.heading }, [
    options.project ? `Feedback for ${options.project}` : 'Send feedback',
  ]);

  const kindButtons = (['bug', 'feedback'] as const).map((value) =>
    h('button', { type: 'button', class: 'fh-seg', role: 'radio', 'data-kind': value }, [
      value === 'bug' ? 'Bug report' : 'Feedback',
    ]),
  );
  const kindGroup = h('div', { class: 'fh-segmented', role: 'radiogroup', 'aria-label': 'Type' }, kindButtons);

  const titleInput = h('input', {
    class: 'fh-input',
    id: ids.title,
    name: 'title',
    type: 'text',
    maxlength: String(TITLE_MAX),
    autocomplete: 'off',
  });
  const bodyInput = h('textarea', {
    class: 'fh-input',
    id: ids.body,
    name: 'body',
    required: true,
    maxlength: String(BODY_MAX),
    'aria-describedby': ids.counter,
  });
  const counter = h('div', { class: 'fh-counter', id: ids.counter }, [`0 / ${BODY_MAX}`]);

  const stars = [1, 2, 3, 4, 5].map((n) =>
    h('button', { type: 'button', class: 'fh-star', role: 'radio', 'aria-label': `${n} star${n === 1 ? '' : 's'}`, 'data-rating': String(n) }, ['★']),
  );
  const ratingField = h('fieldset', { class: 'fh-rating fh-field' }, [
    h('legend', { class: 'fh-label' }, ['Rating ', h('span', { class: 'fh-optional' }, ['(optional)'])]),
    h('div', { class: 'fh-stars', role: 'radiogroup', 'aria-label': 'Rating' }, stars),
  ]);

  const contactInput = h('input', {
    class: 'fh-input',
    id: ids.contact,
    name: 'contact',
    type: 'text',
    maxlength: String(CONTACT_MAX),
    autocomplete: 'email',
    placeholder: 'Email or handle, if you would like a reply',
  });

  const honeypot = h('input', { id: ids.hp, name: 'website', type: 'text', tabindex: '-1', autocomplete: 'off' });
  const honeypotWrap = h('div', { class: 'fh-hp', 'aria-hidden': 'true' }, [
    h('label', { for: ids.hp }, ['Website']),
    honeypot,
  ]);

  const errorBox = h('div', { class: 'fh-error', role: 'alert' });
  const submitButton = h('button', { type: 'submit', class: 'fh-btn' }, ['Send']);
  const status = h('p', { class: 'fh-status', 'aria-live': 'polite' });

  const optional = () => h('span', { class: 'fh-optional' }, [' (optional)']);
  const form = h('form', { class: 'fh-form', novalidate: true, 'aria-labelledby': ids.heading }, [
    heading,
    kindGroup,
    h('div', { class: 'fh-field' }, [h('label', { class: 'fh-label', for: ids.title }, ['Title', optional()]), titleInput]),
    h('div', { class: 'fh-field' }, [h('label', { class: 'fh-label', for: ids.body }, ['Description']), bodyInput, counter]),
    ratingField,
    h('div', { class: 'fh-field' }, [h('label', { class: 'fh-label', for: ids.contact }, ['Contact', optional()]), contactInput]),
    honeypotWrap,
    errorBox,
    h('div', { class: 'fh-actions' }, [submitButton, status]),
  ]);

  const successTitle = h('p', { class: 'fh-success-title' }, ['Thanks! Your feedback was sent.']);
  const againButton = h('button', { type: 'button', class: 'fh-btn fh-btn-secondary' }, ['Send another']);
  const success = h('div', { class: 'fh-success', role: 'status', hidden: true }, [successTitle, againButton]);

  const card = h('div', { class: 'fh-card fh-relative' });
  if (options.onClose) {
    const close = h('button', { type: 'button', class: 'fh-close', 'aria-label': 'Close' }, ['×']);
    close.addEventListener('click', () => options.onClose?.());
    card.append(close);
  }
  card.append(form, success);
  root.append(card);

  function renderKind() {
    for (const btn of kindButtons) btn.setAttribute('aria-checked', String(btn.dataset.kind === kind));
    bodyInput.setAttribute('placeholder', PLACEHOLDERS[kind]);
    ratingField.hidden = kind !== 'feedback';
  }
  function renderRating() {
    for (const star of stars) {
      const n = Number(star.dataset.rating);
      star.classList.toggle('fh-on', rating !== undefined && n <= rating);
      star.setAttribute('aria-checked', String(n === rating));
    }
  }
  function renderCounter() {
    const len = bodyInput.value.length;
    counter.textContent = `${len} / ${BODY_MAX}`;
    counter.classList.toggle('fh-over', len > BODY_MAX);
  }

  for (const btn of kindButtons) {
    btn.addEventListener('click', () => {
      kind = btn.dataset.kind as FeedbackKind;
      renderKind();
    });
  }
  for (const star of stars) {
    star.addEventListener('click', () => {
      const n = Number(star.dataset.rating);
      rating = rating === n ? undefined : n;
      renderRating();
    });
  }
  bodyInput.addEventListener('input', () => {
    renderCounter();
    if (errorBox.textContent) errorBox.textContent = '';
  });

  function prepare() {
    if (prepared) return;
    prepared = true;
    // Failures surface on submit, which prepares again.
    client.prepare().catch(() => {
      prepared = false;
    });
  }
  form.addEventListener('focusin', prepare);

  let observer: IntersectionObserver | undefined;
  if (typeof IntersectionObserver !== 'undefined') {
    observer = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        prepare();
        observer?.disconnect();
      }
    });
    observer.observe(host);
  }

  let sending = false;
  form.addEventListener('submit', async (event) => {
    event.preventDefault();
    if (sending) return;
    errorBox.textContent = '';
    const body = bodyInput.value.trim();
    if (body.length < BODY_MIN) {
      errorBox.textContent = `Please write at least ${BODY_MIN} characters in the description.`;
      bodyInput.setAttribute('aria-invalid', 'true');
      bodyInput.focus();
      return;
    }
    if (body.length > BODY_MAX) {
      errorBox.textContent = `The description can be at most ${BODY_MAX} characters.`;
      bodyInput.setAttribute('aria-invalid', 'true');
      bodyInput.focus();
      return;
    }
    bodyInput.removeAttribute('aria-invalid');

    sending = true;
    submitButton.disabled = true;
    status.textContent = 'Sending...';
    try {
      await client.submit({
        kind,
        title: titleInput.value.trim(),
        body,
        contact: contactInput.value.trim(),
        ...(kind === 'feedback' && rating !== undefined ? { rating } : {}),
        meta: { ...collectMeta(), ...options.meta },
        website: honeypot.value,
      });
      status.textContent = '';
      form.hidden = true;
      success.hidden = false;
      againButton.focus();
      options.onSuccess?.();
    } catch (err) {
      status.textContent = '';
      errorBox.textContent = friendlyError(err);
    } finally {
      sending = false;
      submitButton.disabled = false;
    }
  });

  function reset() {
    form.reset();
    kind = options.kind ?? 'feedback';
    rating = undefined;
    errorBox.textContent = '';
    status.textContent = '';
    bodyInput.removeAttribute('aria-invalid');
    renderKind();
    renderRating();
    renderCounter();
    form.hidden = false;
    success.hidden = true;
  }

  againButton.addEventListener('click', () => {
    reset();
    prepared = false;
    prepare();
    focus();
  });

  function focus() {
    kindButtons.find((b) => b.dataset.kind === kind)?.focus();
  }

  renderKind();
  renderRating();

  return {
    prepare,
    focus,
    reset,
    destroy() {
      observer?.disconnect();
      card.remove();
    },
    root,
  };
}
