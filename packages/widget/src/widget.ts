/**
 * IIFE entry for `/widget.js`. Auto initialises:
 *  - inline forms in every `[data-earful-inline]` element, and
 *  - a floating launcher when the script tag itself carries `data-earful-key`.
 * Also exposes `window.Earful`.
 */
import { createClient, EarfulError } from './client';
import { mount, type MountedForm, type MountOptions } from './form';
import { applyStyles } from './styles';
import type { FeedbackKind } from './client';

const script = (typeof document !== 'undefined' ? document.currentScript : null) as HTMLScriptElement | null;

function scriptOrigin(): string {
  try {
    if (script?.src) return new URL(script.src, location.href).origin;
  } catch {
    // Fall through to the page origin.
  }
  return location.origin;
}

/** Reads `data-earful-<name>`, falling back to the pre-rename `data-fh-<name>`. */
function data(el: HTMLElement | null | undefined, name: string): string | undefined {
  if (!el) return undefined;
  return el.dataset[`earful${name}`] ?? el.dataset[`fh${name}`];
}

function defaultEndpoint(): string {
  return data(script, 'Endpoint') || scriptOrigin();
}

/**
 * Inline elements may come from user-generated content on the host page (many
 * sanitizers keep data-* attributes), so an element can only pick an endpoint
 * on the same origin as the script's own endpoint. Anything else is ignored,
 * otherwise a forum post could mount a lookalike form that reports elsewhere.
 */
function elementEndpoint(el: HTMLElement): string {
  const fallback = defaultEndpoint();
  const wanted = el === script ? undefined : data(el, 'Endpoint');
  if (!wanted) return fallback;
  try {
    if (new URL(wanted, location.href).origin === new URL(fallback, location.href).origin) return wanted;
  } catch {
    // Invalid URL: use the default.
  }
  console.warn('[earful] ignoring data-earful-endpoint on a different origin than widget.js', el);
  return fallback;
}

function parseMeta(raw: string | undefined): Record<string, unknown> | undefined {
  if (!raw) return undefined;
  try {
    const value: unknown = JSON.parse(raw);
    return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined;
  } catch {
    return undefined;
  }
}

function parseKind(raw: string | undefined): FeedbackKind | undefined {
  return raw === 'bug' || raw === 'feedback' ? raw : undefined;
}

function optionsFrom(el: HTMLElement, fallbackKey?: string): MountOptions | null {
  const key = data(el, 'Key') || fallbackKey;
  if (!key) {
    console.warn('[earful] missing data-earful-key on', el);
    return null;
  }
  const opts: MountOptions = { key, endpoint: elementEndpoint(el) };
  const project = data(el, 'Project');
  if (project) opts.project = project;
  const kind = parseKind(data(el, 'Kind'));
  if (kind) opts.kind = kind;
  const meta = parseMeta(data(el, 'Meta'));
  if (meta) opts.meta = meta;
  return opts;
}

/** Mounts inline forms that have not been mounted yet. */
export function mountInline(scope: ParentNode = document): MountedForm[] {
  const mounted: MountedForm[] = [];
  for (const el of Array.from(scope.querySelectorAll<HTMLElement>('[data-earful-inline], [data-fh-inline]'))) {
    if (el.dataset.earfulMounted) continue;
    const opts = optionsFrom(el, data(script, 'Key'));
    if (!opts) continue;
    el.dataset.earfulMounted = 'true';
    mounted.push(mount(el, opts));
  }
  return mounted;
}

export interface LauncherOptions extends MountOptions {
  position?: 'bottom-right' | 'bottom-left';
  label?: string;
}

/** Floating button that toggles a feedback panel. */
export function mountLauncher(opts: LauncherOptions): { open(): void; close(): void; destroy(): void } {
  const host = document.createElement('div');
  host.setAttribute('data-earful-launcher', '');
  document.body.append(host);
  const root = host.attachShadow({ mode: 'open' });
  applyStyles(root);

  const pos = opts.position === 'bottom-left' ? 'earful-pos-left' : 'earful-pos-right';
  const button = document.createElement('button');
  button.type = 'button';
  button.className = `earful-launcher ${pos}`;
  button.textContent = opts.label || 'Feedback';
  button.setAttribute('aria-expanded', 'false');
  button.setAttribute('aria-haspopup', 'dialog');

  const panelHost = document.createElement('div');
  panelHost.className = `earful-panel ${pos}`;
  panelHost.hidden = true;
  panelHost.setAttribute('role', 'dialog');
  panelHost.setAttribute('aria-label', opts.project ? `Feedback for ${opts.project}` : 'Send feedback');
  root.append(button, panelHost);

  let form: MountedForm | undefined;

  function open() {
    if (!form) {
      const inner = document.createElement('div');
      panelHost.append(inner);
      form = mount(inner, { ...opts, onClose: close });
    }
    panelHost.hidden = false;
    button.setAttribute('aria-expanded', 'true');
    form.prepare();
    form.focus();
  }

  function close() {
    panelHost.hidden = true;
    button.setAttribute('aria-expanded', 'false');
    button.focus();
  }

  button.addEventListener('click', () => (panelHost.hidden ? open() : close()));
  root.addEventListener('keydown', (event) => {
    if ((event as KeyboardEvent).key === 'Escape' && !panelHost.hidden) {
      event.stopPropagation();
      close();
    }
  });

  return {
    open,
    close,
    destroy() {
      form?.destroy();
      host.remove();
    },
  };
}

function autoInit() {
  mountInline();
  if (script && data(script, 'Key')) {
    const opts = optionsFrom(script);
    if (opts) {
      const position = data(script, 'Position') === 'bottom-left' ? 'bottom-left' : 'bottom-right';
      const launcher: LauncherOptions = { ...opts, position };
      const label = data(script, 'Label');
      if (label) launcher.label = label;
      mountLauncher(launcher);
    }
  }
}

declare global {
  interface Window {
    Earful?: {
      createClient: typeof createClient;
      mount: typeof mount;
      mountInline: typeof mountInline;
      mountLauncher: typeof mountLauncher;
      EarfulError: typeof EarfulError;
    };
  }
}

if (typeof window !== 'undefined') {
  window.Earful = { createClient, mount, mountInline, mountLauncher, EarfulError };
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', autoInit, { once: true });
  } else {
    autoInit();
  }
}
