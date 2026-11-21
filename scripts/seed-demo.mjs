// Seeds a running earful instance with believable demo data through
// the real public, admin and MCP APIs. Used by scripts/screenshots.mjs, and
// handy on its own for poking at the UI:
//
//   EARFUL_TRUSTED_PROXY_HOPS=1 EARFUL_POW_BITS=8 EARFUL_MIN_FILL_SECS=0 ... earful serve
//   node scripts/seed-demo.mjs http://127.0.0.1:8080 'admin-password'
//
// The server must trust one proxy hop so each simulated player gets its own
// IP via X-Forwarded-For, otherwise the per IP rate limits kick in.

import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';

const DAY = 86_400;

export const PROJECTS = [
  {
    key: 'starfall',
    title: 'Starfall Tactics',
    slug: 'starfall',
    project_type: 'game',
    description: 'Turn based space tactics on Steam. Early Access, Windows, Linux and Steam Deck.',
  },
  {
    key: 'ledgerly',
    title: 'Ledgerly',
    slug: 'ledgerly',
    project_type: 'webapp',
    description: 'Personal finance web app: budgets, recurring bills and bank sync.',
  },
  {
    key: 'tidyctl',
    title: 'tidyctl',
    slug: 'tidyctl',
    project_type: 'service',
    description: 'CLI and daemon that finds and cleans up stale cloud resources.',
  },
];

// Imported history (via MCP create_feedback, backdated) so the dashboard has a
// month of activity. [project, kind, daysAgo, title, body, extra]
const HISTORY = [
  ['starfall', 'bug', 29, 'Game crashes when opening the hangar', 'Clicking the hangar tab right after a mission ends crashes to desktop every time. Started after the 0.9.2 patch.', { meta: { version: '0.9.2', platform: 'windows', gpu: 'RTX 3070' }, status: 'resolved', admin_note: 'Null ship loadout after mission end. Fixed in 0.9.3.' }],
  ['starfall', 'feedback', 27, 'Love the new campaign map', 'The new star map is gorgeous and much easier to read. The fog of war reveal animation is a nice touch.', { rating: 5, meta: { version: '0.9.2', platform: 'steam-deck' } }],
  ['starfall', 'bug', 25, 'Stutter when zooming the battle grid', 'Frame rate drops to ~20 fps when zooming in and out quickly on large maps. Steam Deck, default settings.', { meta: { version: '0.9.3', platform: 'steam-deck', fps: '20' }, status: 'triaged' }],
  ['ledgerly', 'bug', 24, 'CSV import ignores negative amounts', 'Importing my bank CSV turns every negative amount into a positive one, so refunds show up as expenses.', { meta: { version: '2.14.0', browser: 'Firefox 132' }, status: 'resolved', admin_note: 'Locale parsing of the minus sign. Fixed in 2.14.1.' }],
  ['starfall', 'feedback', 22, 'Please add controller remapping', 'Would love to rebind the gamepad buttons. Confirm on A and cancel on B is backwards for me.', { rating: 4, meta: { version: '0.9.3', platform: 'linux', input: 'xbox controller' }, status: 'triaged' }],
  ['tidyctl', 'bug', 21, 'Dry run deletes snapshots anyway', 'Running `tidyctl prune --dry-run` still deleted two EBS snapshots older than 30 days. Expected a report only.', { meta: { version: '1.4.0', os: 'ubuntu 24.04' }, status: 'resolved', admin_note: 'Critical. Flag was not passed to the snapshot pruner. Hotfix 1.4.1.' }],
  ['ledgerly', 'feedback', 20, 'Dark mode for the dashboard', 'The dashboard is very bright at night. A dark theme would be great, especially for the charts.', { rating: 4, meta: { version: '2.14.1' } }],
  ['starfall', 'bug', 18, 'Save file lost after power cut', 'My laptop died mid autosave and now the save slot shows as empty. 14 hours of progress gone.', { meta: { version: '0.9.3', platform: 'windows' }, status: 'triaged', admin_note: 'Write to temp file and rename atomically.' }],
  ['ledgerly', 'bug', 17, 'Recurring bill shows twice in March', 'My rent shows up twice in the March forecast. It only happens for bills that fall on the 31st.', { meta: { version: '2.15.0', browser: 'Safari 18' } }],
  ['tidyctl', 'feedback', 16, 'Support for GCP would be amazing', 'We moved half of our infra to GCP. Would love tidyctl to scan GCE disks and snapshots too.', { rating: 5, meta: { version: '1.4.1' } }],
  ['starfall', 'feedback', 14, 'Enemy AI is too passive on hard', 'On hard difficulty the AI just waits behind asteroids. It should flank or at least use its carriers.', { rating: 3, meta: { version: '0.9.4', difficulty: 'hard' } }],
  ['starfall', 'bug', 12, 'Music cuts out after alt-tab', 'Alt-tabbing out and back in mutes the music, sound effects still work. Restarting the game fixes it.', { meta: { version: '0.9.4', platform: 'windows' }, status: 'resolved' }],
  ['ledgerly', 'feedback', 11, 'Export budgets to PDF', 'I share a monthly budget summary with my partner. A PDF export of the budget page would save me screenshots.', { rating: 4 }],
  ['tidyctl', 'bug', 10, 'Panics on empty AWS profile', 'tidyctl panics with "index out of range" when ~/.aws/config has a profile without a region.', { meta: { version: '1.5.0', os: 'macOS 15' }, status: 'triaged' }],
  ['starfall', 'bug', 9, 'Tooltip text overflows on 4K', 'At 4K with 200% UI scale the weapon tooltips overflow their box and overlap the minimap.', { meta: { version: '0.9.4', resolution: '3840x2160' } }],
  ['ledgerly', 'bug', 8, 'Bank sync stuck on "connecting"', 'The bank sync spinner never finishes for my credit union. Other accounts sync fine.', { meta: { version: '2.15.2', browser: 'Chrome 141' }, status: 'triaged' }],
  ['starfall', 'feedback', 7, 'The soundtrack is fantastic', 'Just wanted to say the soundtrack is fantastic. Is there a way to buy it on Bandcamp?', { rating: 5 }],
  ['tidyctl', 'feedback', 6, 'JSON output for CI pipelines', 'An `--output json` flag would let us fail CI when tidyctl finds orphaned resources.', { rating: 4, meta: { version: '1.5.0' } }],
  ['starfall', 'bug', 5, 'Freezes on the loading screen', 'The game gets stuck on the loading screen for the Kepler mission. Have to kill it from task manager.', { meta: { version: '0.9.5', platform: 'windows', gpu: 'RX 6700' } }],
  ['ledgerly', 'feedback', 4, 'Split transactions', 'Being able to split one transaction across two categories (groceries and household) is the one thing I miss from my old app.', { rating: 4 }],
  ['starfall', 'feedback', 3, 'Ship customization is great, more colors please', 'Loving the ship customization. More paint colors and decals would make fleets feel more personal.', { rating: 5, meta: { version: '0.9.5' } }],
  ['tidyctl', 'bug', 2, 'Wrong cost estimate for gp3 volumes', 'The savings report prices gp3 volumes at gp2 rates, so the estimate is about 20% too high.', { meta: { version: '1.5.1' } }],
  ['ledgerly', 'bug', 1, 'Login loop on iOS Safari', 'After entering my password on iPhone, the page reloads and asks me to log in again. Works on desktop.', { meta: { version: '2.16.0', browser: 'iOS Safari 18' } }],
  ['starfall', 'bug', 28, 'Camera snaps back after rotating', 'Rotating the camera with Q and E snaps it back to the default angle after a second.', { meta: { version: '0.9.2' }, status: 'resolved' }],
  ['ledgerly', 'feedback', 26, 'Keyboard shortcuts please', 'Power user here: shortcuts for adding a transaction and switching months would be lovely.', { rating: 4 }],
  ['tidyctl', 'bug', 23, 'Typo in the help output', 'The help for `prune` says "snapshtos" instead of "snapshots".', { meta: { version: '1.4.0' }, status: 'resolved' }],
  ['starfall', 'feedback', 19, 'Tutorial is too long', 'The tutorial explains movement three times. Let experienced players skip ahead.', { rating: 3 }],
  ['starfall', 'bug', 15, 'Crash when loading an old save', 'Loading a save from 0.9.1 crashes the game right after the loading bar fills up.', { meta: { version: '0.9.4', platform: 'linux' }, status: 'triaged' }],
  ['ledgerly', 'bug', 13, 'Budget rollover off by one cent', 'Rolling over the groceries budget leaves 0.01 behind every month. Rounding issue?', { meta: { version: '2.15.1' }, status: 'resolved' }],
  ['tidyctl', 'feedback', 12, 'Great tool, saved us $400/month', 'Ran it once on our staging account and found 1.2 TB of forgotten snapshots. Thank you!', { rating: 5 }],
  ['starfall', 'bug', 11, 'Laggy menus on the Steam Deck', 'The main menu and the hangar UI feel laggy on the Deck, input takes half a second to register.', { meta: { version: '0.9.4', platform: 'steam-deck' } }],
  ['ledgerly', 'feedback', 9, 'Shared budgets with a partner', 'Would pay for a shared household budget that two accounts can edit.', { rating: 5 }],
  ['starfall', 'feedback', 8, 'More save slots', 'Three save slots is not enough when experimenting with different fleet builds.', { rating: 4 }],
  ['tidyctl', 'bug', 7, 'Scan hangs on accounts with 10k volumes', 'The scan never finishes on our largest account. Looks like pagination stops after the first page.', { meta: { version: '1.5.0' }, status: 'triaged' }],
  ['starfall', 'bug', 6, 'Sound effects too loud compared to music', 'Weapon sound effects are way louder than the music, even with SFX at 50%.', { meta: { version: '0.9.5' } }],
  ['ledgerly', 'bug', 5, 'Date picker shows wrong week start', 'The date picker starts weeks on Sunday even though my locale is German.', { meta: { version: '2.15.2', browser: 'Chrome 141' } }],
  ['starfall', 'bug', 4, 'Achievement popup covers the end turn button', 'The Steam achievement popup sits right over the end turn button for a few seconds.', { meta: { version: '0.9.5' } }],
  ['tidyctl', 'feedback', 3, 'Homebrew formula', 'A Homebrew tap would make installing and updating on macOS much easier.', { rating: 4 }],
  ['ledgerly', 'feedback', 2, 'Love the new onboarding', 'The new onboarding flow got my accounts connected in two minutes. Really nice work.', { rating: 5 }],
  ['starfall', 'feedback', 1, 'Steam Deck verified when?', 'Runs great on my Deck already. Any plans to get the Verified badge?', { rating: 5, meta: { platform: 'steam-deck' } }],
];

// Live submissions through the public ingestion API (real PoW), one player IP each.
const LIVE = [
  ['starfall', 'bug', 'Inventory crash during the dragon boss', 'Opening the inventory during the dragon boss fight crashes the game to desktop. Happens 3 out of 3 times on the Steam Deck.', { contact: 'nova_pilot#2231', meta: { version: '0.9.5', platform: 'steam-deck', build: '5521' } }],
  ['starfall', 'bug', 'Hard lock when the hangar loads', 'The whole game hangs and gets stuck after the hangar loads, only a reboot helps.', { meta: { version: '0.9.5', platform: 'steam-deck' } }],
  ['starfall', 'feedback', 'Add an undo for the last move', 'Misclicks happen a lot on the Deck touchscreen. An undo for the last move (maybe only on easy) would help.', { rating: 4, meta: { version: '0.9.5', platform: 'steam-deck' } }],
  ['ledgerly', 'bug', 'Charts are blank in Firefox', 'The spending charts on the dashboard stay blank in Firefox with strict tracking protection.', { contact: 'maria@example.com', meta: { version: '2.16.0', browser: 'Firefox 133' } }],
  ['tidyctl', 'bug', 'Daemon uses 100% CPU when idle', 'The tidyctl daemon pins one core at 100% even with nothing scheduled. Looks like a busy loop.', { meta: { version: '1.5.1', os: 'debian 12' } }],
];

// The same report from three different players: merged into one row with dup_count 2.
const DUPLICATE = ['starfall', 'bug', 'Achievements do not unlock', 'Finished the first campaign but the "First Contact" achievement did not unlock on Steam.', { meta: { version: '0.9.5' } }];

const SPAM = [
  ['ledgerly', 'feedback', 'Best casino bonus 2026', 'Visit https://casino.example https://bonus.example https://bit.ly/free-spins for free bitcoin and guaranteed profit!!!', {}],
  ['starfall', 'feedback', 'SEO SERVICES', 'WE SELL BACKLINKS AND SEO SERVICE FOR YOUR GAME WEBSITE, WHATSAPP ME FOR CHEAP PRICES TODAY', {}],
];

function workBits(challenge, nonce) {
  const hash = createHash('sha256').update(`${challenge}:${nonce}`).digest();
  let bits = 0;
  for (const byte of hash) {
    if (byte === 0) {
      bits += 8;
      continue;
    }
    bits += Math.clz32(byte) - 24;
    break;
  }
  return bits;
}

function solve(challenge, bits) {
  for (let n = 0; ; n++) if (workBits(challenge, String(n)) >= bits) return String(n);
}

let ipCounter = 10;
const nextIp = () => `203.0.113.${ipCounter++}`;

export async function seed(base, password) {
  const json = async (res, what) => {
    if (!res.ok && res.status !== 202) throw new Error(`${what}: HTTP ${res.status} ${await res.text()}`);
    return res.status === 204 || res.status === 202 ? null : res.json();
  };

  const login = await fetch(`${base}/api/admin/login`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ password }),
  });
  if (login.status !== 204) throw new Error(`login failed: ${login.status}`);
  const cookie = login.headers.get('set-cookie').split(';')[0];
  const admin = (path, init = {}) =>
    fetch(`${base}${path}`, {
      ...init,
      headers: { 'content-type': 'application/json', cookie, ...(init.headers ?? {}) },
    });

  const projects = {};
  for (const p of PROJECTS) {
    const { key, ...body } = p;
    projects[key] = await json(await admin('/api/admin/projects', { method: 'POST', body: JSON.stringify(body) }), 'create project');
  }

  // Tokens: one for an agent, one read only bot.
  const agent = await json(
    await admin('/api/admin/tokens', { method: 'POST', body: JSON.stringify({ name: 'claude-code laptop', scope: 'write' }) }),
    'create token',
  );
  await json(await admin('/api/admin/tokens', { method: 'POST', body: JSON.stringify({ name: 'release-notes bot', scope: 'read' }) }), 'create token');

  // Backdated history via MCP, exactly like an agent importing old reports.
  let rpcId = 1;
  const mcp = async (name, args) => {
    const res = await fetch(`${base}/mcp`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', authorization: `Bearer ${agent.token}` },
      body: JSON.stringify({ jsonrpc: '2.0', id: rpcId++, method: 'tools/call', params: { name, arguments: args } }),
    });
    const out = await json(res, `mcp ${name}`);
    if (out.error || out.result?.isError) throw new Error(`mcp ${name}: ${JSON.stringify(out.error ?? out.result)}`);
    return out.result;
  };
  const now = Math.floor(Date.now() / 1000);
  for (const [project, kind, daysAgo, title, body, extra] of HISTORY) {
    const jitter = Math.floor((daysAgo * 7919) % 36_000);
    await mcp('create_feedback', {
      project: projects[project].slug,
      kind,
      title,
      body,
      created_at: String(now - daysAgo * DAY - jitter),
      ...extra,
    });
  }

  // Live public submissions with real proof of work.
  const submit = async (project, kind, title, body, extra = {}, ip = nextIp(), overrides = {}) => {
    const key = projects[project].ingest_key;
    const ch = await json(await fetch(`${base}/api/v1/challenge?key=${key}`, { headers: { 'x-forwarded-for': ip } }), 'challenge');
    const res = await fetch(`${base}/api/v1/feedback`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'x-forwarded-for': ip },
      body: JSON.stringify({ key, kind, title, body, source: 'web', challenge: ch.challenge, nonce: solve(ch.challenge, ch.bits), ...extra, ...overrides }),
    });
    return res.status;
  };
  for (const [project, kind, title, body, extra] of LIVE) await submit(project, kind, title, body, extra);
  for (let i = 0; i < 3; i++) await submit(...DUPLICATE);
  for (const [project, kind, title, body, extra] of SPAM) await submit(project, kind, title, body, extra);

  // A spammer repeating itself: an exact repeat (dropped) and a near copy (quarantined).
  const repeater = nextIp();
  const pitch = 'Hello admin, we offer professional game marketing and streamer outreach for indie studios. Reply to get our price list and portfolio, limited slots available this month.';
  await submit('starfall', 'feedback', 'Marketing offer', pitch, {}, repeater);
  await submit('starfall', 'feedback', 'Marketing offer', pitch, {}, repeater);
  await submit('starfall', 'feedback', 'Marketing offer', pitch.replace('this month', 'this week'), {}, repeater);

  // Blocked attempts for the dashboard: honeypot hits, weak proof of work, a flood.
  for (let i = 0; i < 4; i++) await submit('starfall', 'feedback', 'hello', 'cheap followers here', {}, nextIp(), { website: 'https://spam.example' });
  for (let i = 0; i < 3; i++) await submit('ledgerly', 'bug', 'x', 'scripted submission', {}, nextIp(), { nonce: 'not-a-nonce' });
  const flooder = nextIp();
  const floods = [
    'Game is bad', 'Game is really bad', 'Game is really really bad', 'Worst game ever made', 'Refund now',
    'Please refund', 'This is a scam', 'Fix your game',
  ];
  for (const text of floods) await submit('starfall', 'feedback', text, `${text}. Seriously.`, {}, flooder);

  // A few status changes so the tabs are not all "new".
  const list = await json(await admin('/api/admin/feedback?status=any&limit=200'), 'list');
  const byTitle = (t) => list.items.find((f) => f.title === t);
  const set = async (title, patch) => {
    const item = byTitle(title);
    if (item) await json(await admin(`/api/admin/feedback/${item.id}`, { method: 'PATCH', body: JSON.stringify(patch) }), 'patch');
  };
  await set('Game is bad', { status: 'archived', admin_note: 'Not actionable.' });
  await set('Game is really bad', { status: 'archived' });
  await set('Game is really really bad', { status: 'archived' });
  // The polite first marketing pitch slipped past the heuristics; the admin marks it by hand.
  for (const item of list.items.filter((f) => f.title === 'Marketing offer' && f.status !== 'spam')) {
    await json(await admin(`/api/admin/feedback/${item.id}`, { method: 'PATCH', body: JSON.stringify({ status: 'spam' }) }), 'patch');
  }
  await set('Charts are blank in Firefox', { status: 'triaged', admin_note: 'Probably the canvas fingerprinting block. Try SVG fallback.' });

  // Let the background worker embed everything (similar items need vectors).
  for (let i = 0; i < 50; i++) {
    const stats = await json(await admin('/api/admin/stats'), 'stats');
    if (stats.embeddings.pending === 0) break;
    await new Promise((r) => setTimeout(r, 200));
  }

  return { projects, cookie };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [base = 'http://127.0.0.1:8080', password = process.env.EARFUL_DEMO_PASSWORD] = process.argv.slice(2);
  if (!password) throw new Error('usage: node scripts/seed-demo.mjs <base-url> <admin-password>');
  await seed(base, password);
  console.log(`seeded ${base}`);
}
