/** Styles for the Shadow DOM. Kept as a string so the bundle stays one file. */
export const css = `
:host {
  --fh-bg: #ffffff;
  --fh-fg: #1d1f23;
  --fh-muted: #5f6672;
  --fh-border: #d6d9de;
  --fh-field: #f7f8fa;
  --fh-accent: #3b5bdb;
  --fh-accent-fg: #ffffff;
  --fh-danger: #c92a2a;
  --fh-success: #2b8a3e;
  --fh-ring: rgba(59, 91, 219, 0.35);
  --fh-radius: 10px;
  all: initial;
  color: var(--fh-fg);
  font: 14px/1.45 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
}
@media (prefers-color-scheme: dark) {
  :host {
    --fh-bg: #1b1d22;
    --fh-fg: #e8eaee;
    --fh-muted: #a0a7b4;
    --fh-border: #343842;
    --fh-field: #23262d;
    --fh-accent: #7b93ff;
    --fh-accent-fg: #0d1020;
    --fh-danger: #ff8787;
    --fh-success: #69db7c;
    --fh-ring: rgba(123, 147, 255, 0.4);
  }
}
*, *::before, *::after { box-sizing: border-box; }
.fh-card {
  background: var(--fh-bg);
  color: var(--fh-fg);
  border: 1px solid var(--fh-border);
  border-radius: var(--fh-radius);
  padding: 18px;
  max-width: 520px;
}
.fh-form { display: grid; gap: 12px; margin: 0; }
.fh-heading { font-size: 17px; font-weight: 650; margin: 0; padding-right: 28px; }
.fh-field { display: grid; gap: 4px; }
.fh-label { font-weight: 550; font-size: 13px; }
.fh-optional { color: var(--fh-muted); font-weight: 400; }
input.fh-input, textarea.fh-input {
  font: inherit;
  color: inherit;
  background: var(--fh-field);
  border: 1px solid var(--fh-border);
  border-radius: 8px;
  padding: 8px 10px;
  width: 100%;
}
textarea.fh-input { min-height: 110px; resize: vertical; }
.fh-input:focus-visible, .fh-seg:focus-visible, .fh-star:focus-visible, .fh-btn:focus-visible, .fh-close:focus-visible, .fh-launcher:focus-visible {
  outline: 2px solid var(--fh-accent);
  outline-offset: 2px;
  box-shadow: 0 0 0 4px var(--fh-ring);
}
.fh-counter { color: var(--fh-muted); font-size: 12px; justify-self: end; }
.fh-counter.fh-over { color: var(--fh-danger); }
.fh-segmented { display: inline-flex; border: 1px solid var(--fh-border); border-radius: 8px; overflow: hidden; width: max-content; }
.fh-seg { font: inherit; color: inherit; background: transparent; border: 0; padding: 6px 14px; cursor: pointer; }
.fh-seg + .fh-seg { border-left: 1px solid var(--fh-border); }
.fh-seg[aria-checked="true"] { background: var(--fh-accent); color: var(--fh-accent-fg); }
.fh-stars { display: inline-flex; gap: 2px; }
.fh-star { font-size: 22px; line-height: 1; background: none; border: 0; padding: 2px; cursor: pointer; color: var(--fh-border); border-radius: 4px; }
.fh-star.fh-on { color: #f59f00; }
.fh-rating[hidden] { display: none; }
fieldset.fh-rating { border: 0; margin: 0; padding: 0; min-width: 0; }
fieldset.fh-rating legend { padding: 0; margin-bottom: 4px; }
.fh-hp { position: absolute !important; left: -10000px !important; width: 1px; height: 1px; overflow: hidden; }
.fh-error { color: var(--fh-danger); font-size: 13px; min-height: 0; }
.fh-error:empty { display: none; }
.fh-actions { display: flex; align-items: center; gap: 10px; }
.fh-btn { font: inherit; font-weight: 600; background: var(--fh-accent); color: var(--fh-accent-fg); border: 0; border-radius: 8px; padding: 8px 16px; cursor: pointer; }
.fh-btn[disabled] { opacity: 0.6; cursor: progress; }
.fh-btn-secondary { background: transparent; color: var(--fh-accent); border: 1px solid var(--fh-border); }
.fh-status { color: var(--fh-muted); font-size: 13px; margin: 0; }
.fh-success { display: grid; gap: 12px; justify-items: start; }
.fh-success-title { color: var(--fh-success); font-weight: 650; font-size: 16px; margin: 0; }
.fh-launcher {
  position: fixed; bottom: 20px; z-index: 2147483000;
  font: inherit; font-weight: 600; background: var(--fh-accent); color: var(--fh-accent-fg);
  border: 0; border-radius: 999px; padding: 10px 18px; cursor: pointer;
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.18);
}
.fh-pos-right { right: 20px; }
.fh-pos-left { left: 20px; }
.fh-panel {
  position: fixed; bottom: 72px; z-index: 2147483001;
  width: min(380px, calc(100vw - 24px)); max-height: calc(100vh - 96px); overflow: auto;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
}
.fh-panel[hidden] { display: none; }
.fh-panel.fh-pos-right { right: 12px; }
.fh-panel.fh-pos-left { left: 12px; }
.fh-close {
  position: absolute; top: 10px; right: 10px; background: none; border: 0; color: var(--fh-muted);
  font-size: 20px; line-height: 1; padding: 4px 8px; cursor: pointer; border-radius: 6px;
}
.fh-relative { position: relative; }
@media (prefers-reduced-motion: no-preference) {
  .fh-panel { animation: fh-in 140ms ease-out; }
  @keyframes fh-in { from { opacity: 0; transform: translateY(8px); } to { opacity: 1; transform: none; } }
}
`;

/** Attaches the stylesheet, preferring constructable stylesheets. */
export function applyStyles(root: ShadowRoot): void {
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(css);
    root.adoptedStyleSheets = [...root.adoptedStyleSheets, sheet];
    return;
  } catch {
    // Older engines: fall back to a style element inside the shadow root.
  }
  const style = document.createElement('style');
  style.textContent = css;
  root.appendChild(style);
}
