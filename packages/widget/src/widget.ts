/**
 * IIFE entry for `/widget.js`. Auto initialises:
 *  - inline forms in every `[data-fh-inline]` element, and
 *  - a floating launcher when the script tag itself carries `data-fh-key`.
 * Also exposes `window.FeedbackHarvest`.
 */
import { createClient, FeedbackHarvestError } from './client';
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

function defaultEndpoint(): string {
  return script?.dataset.fhEndpoint || scriptOrigin();
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
  const key = el.dataset.fhKey || fallbackKey;
  if (!key) {
    console.warn('[feedback-harvest] missing data-fh-key on', el);
    return null;
  }
  const opts: MountOptions = { key, endpoint: el.dataset.fhEndpoint || defaultEndpoint() };
  if (el.dataset.fhProject) opts.project = el.dataset.fhProject;
  const kind = parseKind(el.dataset.fhKind);
  if (kind) opts.kind = kind;
  const meta = parseMeta(el.dataset.fhMeta);
  if (meta) opts.meta = meta;
  return opts;
}

/** Mounts inline forms that have not been mounted yet. */
export function mountInline(scope: ParentNode = document): MountedForm[] {
  const mounted: MountedForm[] = [];
  for (const el of Array.from(scope.querySelectorAll<HTMLElement>('[data-fh-inline]'))) {
    if (el.dataset.fhMounted) continue;
    const opts = optionsFrom(el, script?.dataset.fhKey);
    if (!opts) continue;
    el.dataset.fhMounted = 'true';
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
  host.setAttribute('data-fh-launcher', '');
  document.body.append(host);
  const root = host.attachShadow({ mode: 'open' });
  applyStyles(root);

  const pos = opts.position === 'bottom-left' ? 'fh-pos-left' : 'fh-pos-right';
  const button = document.createElement('button');
  button.type = 'button';
  button.className = `fh-launcher ${pos}`;
  button.textContent = opts.label || 'Feedback';
  button.setAttribute('aria-expanded', 'false');
  button.setAttribute('aria-haspopup', 'dialog');

  const panelHost = document.createElement('div');
  panelHost.className = `fh-panel ${pos}`;
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
  if (script?.dataset.fhKey) {
    const opts = optionsFrom(script);
    if (opts) {
      const position = script.dataset.fhPosition === 'bottom-left' ? 'bottom-left' : 'bottom-right';
      const launcher: LauncherOptions = { ...opts, position };
      if (script.dataset.fhLabel) launcher.label = script.dataset.fhLabel;
      mountLauncher(launcher);
    }
  }
}

declare global {
  interface Window {
    FeedbackHarvest?: {
      createClient: typeof createClient;
      mount: typeof mount;
      mountInline: typeof mountInline;
      mountLauncher: typeof mountLauncher;
      FeedbackHarvestError: typeof FeedbackHarvestError;
    };
  }
}

if (typeof window !== 'undefined') {
  window.FeedbackHarvest = { createClient, mount, mountInline, mountLauncher, FeedbackHarvestError };
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', autoInit, { once: true });
  } else {
    autoInit();
  }
}
