// Regenerates raster icons from the hand-written SVG sources.
//
//   node scripts/icons.mjs
//
// Sources: apps/admin/public/{logo,favicon}.svg and docs/assets/logo-light.svg.
// Outputs: favicon.ico (16/32/48), apple-touch-icon.png, icon-192.png,
// icon-512.png and og.png in apps/admin/public/. Uses the e2e package's
// Playwright to rasterize, so no extra dependencies.

import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(join(root, 'e2e', 'package.json'));
const { chromium } = require('@playwright/test');

const pub = join(root, 'apps', 'admin', 'public');
const dataUri = (path) => `data:image/svg+xml;base64,${readFileSync(path).toString('base64')}`;
const logo = dataUri(join(pub, 'logo.svg'));
const favicon = dataUri(join(pub, 'favicon.svg'));
const lockup = dataUri(join(root, 'docs', 'assets', 'logo-light.svg'));

const PAGE = '#f9f9f7';
const MUTED = '#52514e';

const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 1 });

/** Renders `html` into a size x size (or w x h) transparent or opaque PNG. */
async function shot(html, width, height = width) {
  await page.setViewportSize({ width, height });
  await page.setContent(`<!doctype html><html><body style="margin:0;width:${width}px;height:${height}px;overflow:hidden">${html}</body></html>`);
  await page.evaluate(() => Promise.all([...document.images].map((i) => i.decode())));
  return page.screenshot({ omitBackground: true, clip: { x: 0, y: 0, width, height } });
}

const img = (src, size) => `<img src="${src}" width="${size}" height="${size}" style="display:block">`;

// Favicon sizes use the simplified mark, transparent background.
const icoPngs = [];
for (const size of [16, 32, 48]) icoPngs.push({ size, png: await shot(img(favicon, size), size) });
writeFileSync(join(pub, 'favicon.ico'), ico(icoPngs));

// App icons: full bleed page colour (iOS paints transparency black) with the
// mark inside the maskable safe zone.
for (const [name, size] of [['apple-touch-icon.png', 180], ['icon-192.png', 192], ['icon-512.png', 512]]) {
  const inner = Math.round(size * 0.7);
  const html = `<div style="width:${size}px;height:${size}px;background:${PAGE};display:grid;place-items:center">${img(logo, inner)}</div>`;
  writeFileSync(join(pub, name), await shot(html, size));
}

// Open Graph card.
const og = `<div style="width:1200px;height:630px;background:${PAGE};display:flex;flex-direction:column;justify-content:center;padding:0 110px;box-sizing:border-box;font-family:system-ui,-apple-system,'Segoe UI',Helvetica,Arial,sans-serif">
  <img src="${lockup}" style="width:770px;height:auto;display:block">
  <p style="margin:44px 0 0;font-size:38px;line-height:1.35;color:${MUTED};max-width:900px">Self-hosted feedback and bug reports for your games, apps and services. One binary, SQLite, smart anti-spam, semantic search.</p>
</div>`;
writeFileSync(join(pub, 'og.png'), await shot(og, 1200, 630));

await browser.close();
console.log('icons written to', pub);

/** Builds an ICO container holding PNG frames (supported by every current browser). */
function ico(frames) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(frames.length, 4);
  const entries = [];
  let offset = 6 + 16 * frames.length;
  for (const { size, png } of frames) {
    const e = Buffer.alloc(16);
    e.writeUInt8(size >= 256 ? 0 : size, 0);
    e.writeUInt8(size >= 256 ? 0 : size, 1);
    e.writeUInt8(0, 2);
    e.writeUInt8(0, 3);
    e.writeUInt16LE(1, 4);
    e.writeUInt16LE(32, 6);
    e.writeUInt32LE(png.length, 8);
    e.writeUInt32LE(offset, 12);
    offset += png.length;
    entries.push(e);
  }
  return Buffer.concat([header, ...entries, ...frames.map((f) => f.png)]);
}
