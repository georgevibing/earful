/** Styles for the Shadow DOM. Kept as a string so the bundle stays one file. */
export const css = `
:host {
  --earful-bg: #ffffff;
  --earful-fg: #1d1f23;
  --earful-muted: #5f6672;
  --earful-border: #d6d9de;
  --earful-field: #f7f8fa;
  --earful-accent: #3b5bdb;
  --earful-accent-fg: #ffffff;
  --earful-danger: #c92a2a;
  --earful-success: #2b8a3e;
  --earful-ring: rgba(59, 91, 219, 0.35);
  --earful-radius: 10px;
  all: initial;
  color: var(--earful-fg);
  font: 14px/1.45 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
}
@media (prefers-color-scheme: dark) {
  :host {
    --earful-bg: #1b1d22;
    --earful-fg: #e8eaee;
    --earful-muted: #a0a7b4;
    --earful-border: #343842;
    --earful-field: #23262d;
    --earful-accent: #7b93ff;
    --earful-accent-fg: #0d1020;
    --earful-danger: #ff8787;
    --earful-success: #69db7c;
    --earful-ring: rgba(123, 147, 255, 0.4);
  }
}
*, *::before, *::after { box-sizing: border-box; }
.earful-card {
  background: var(--earful-bg);
  color: var(--earful-fg);
  border: 1px solid var(--earful-border);
  border-radius: var(--earful-radius);
  padding: 18px;
  max-width: 520px;
}
.earful-form { display: grid; gap: 12px; margin: 0; }
.earful-heading { font-size: 17px; font-weight: 650; margin: 0; padding-right: 28px; display: flex; align-items: center; gap: 8px; }
.earful-mark { width: 22px; height: 22px; flex: none; }
.earful-field { display: grid; gap: 4px; }
.earful-label { font-weight: 550; font-size: 13px; }
.earful-optional { color: var(--earful-muted); font-weight: 400; }
input.earful-input, textarea.earful-input {
  font: inherit;
  color: inherit;
  background: var(--earful-field);
  border: 1px solid var(--earful-border);
  border-radius: 8px;
  padding: 8px 10px;
  width: 100%;
}
textarea.earful-input { min-height: 110px; resize: vertical; }
.earful-input:focus-visible, .earful-seg:focus-visible, .earful-star:focus-visible, .earful-btn:focus-visible, .earful-close:focus-visible, .earful-launcher:focus-visible {
  outline: 2px solid var(--earful-accent);
  outline-offset: 2px;
  box-shadow: 0 0 0 4px var(--earful-ring);
}
.earful-counter { color: var(--earful-muted); font-size: 12px; justify-self: end; }
.earful-counter.earful-over { color: var(--earful-danger); }
.earful-segmented { display: inline-flex; border: 1px solid var(--earful-border); border-radius: 8px; overflow: hidden; width: max-content; }
.earful-seg { font: inherit; color: inherit; background: transparent; border: 0; padding: 6px 14px; cursor: pointer; }
.earful-seg + .earful-seg { border-left: 1px solid var(--earful-border); }
.earful-seg[aria-checked="true"] { background: var(--earful-accent); color: var(--earful-accent-fg); }
.earful-stars { display: inline-flex; gap: 2px; }
.earful-star { font-size: 22px; line-height: 1; background: none; border: 0; padding: 2px; cursor: pointer; color: var(--earful-border); border-radius: 4px; }
.earful-star.earful-on { color: #f59f00; }
/* Class display rules would otherwise beat the [hidden] attribute. */
[hidden] { display: none !important; }
.earful-rating[hidden] { display: none; }
fieldset.earful-rating { border: 0; margin: 0; padding: 0; min-width: 0; }
fieldset.earful-rating legend { padding: 0; margin-bottom: 4px; }
.earful-hp { position: absolute !important; left: -10000px !important; width: 1px; height: 1px; overflow: hidden; }
.earful-error { color: var(--earful-danger); font-size: 13px; min-height: 0; }
.earful-error:empty { display: none; }
.earful-actions { display: flex; align-items: center; gap: 10px; }
.earful-btn { font: inherit; font-weight: 600; background: var(--earful-accent); color: var(--earful-accent-fg); border: 0; border-radius: 8px; padding: 8px 16px; cursor: pointer; }
.earful-btn[disabled] { opacity: 0.6; cursor: progress; }
.earful-btn-secondary { background: transparent; color: var(--earful-accent); border: 1px solid var(--earful-border); }
.earful-status { color: var(--earful-muted); font-size: 13px; margin: 0; }
.earful-success { display: grid; gap: 12px; justify-items: start; }
.earful-success-title { color: var(--earful-success); font-weight: 650; font-size: 16px; margin: 0; }
.earful-launcher {
  position: fixed; bottom: 20px; z-index: 2147483000;
  font: inherit; font-weight: 600; background: var(--earful-accent); color: var(--earful-accent-fg);
  border: 0; border-radius: 999px; padding: 10px 18px; cursor: pointer;
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.18);
}
.earful-pos-right { right: 20px; }
.earful-pos-left { left: 20px; }
.earful-panel {
  position: fixed; bottom: 72px; z-index: 2147483001;
  width: min(380px, calc(100vw - 24px)); max-height: calc(100vh - 96px); overflow: auto;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
}
.earful-panel[hidden] { display: none; }
.earful-panel.earful-pos-right { right: 12px; }
.earful-panel.earful-pos-left { left: 12px; }
.earful-close {
  position: absolute; top: 10px; right: 10px; background: none; border: 0; color: var(--earful-muted);
  font-size: 20px; line-height: 1; padding: 4px 8px; cursor: pointer; border-radius: 6px;
}
.earful-relative { position: relative; }
@media (prefers-reduced-motion: no-preference) {
  .earful-panel { animation: earful-in 140ms ease-out; }
  @keyframes earful-in { from { opacity: 0; transform: translateY(8px); } to { opacity: 1; transform: none; } }
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
