// Takes 800x600 screenshots of WPT files in headless Chromium, the pixels
// the WPT runner compares Xerune against.
//
//   node chromium_screenshots.mjs WPT_DIR OUT_DIR < paths.txt
//
// Reads WPT-relative paths from stdin and writes OUT_DIR/<path>.png. Files are
// served over HTTP from WPT_DIR so root-relative URLs (/fonts/ahem.css,
// /css/reference/...) resolve. Every system font is replaced by Ahem through a
// private fontconfig file, so text renders like in the runner, which only has
// Ahem. Needs Node 22+ (built-in WebSocket) and `chromium` on PATH (or
// $CHROMIUM).

import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, extname, join, normalize, resolve } from 'node:path';

const [wptDir, outDir] = process.argv.slice(2).map((p) => resolve(p));
if (!wptDir || !outDir) {
  console.error('usage: node chromium_screenshots.mjs WPT_DIR OUT_DIR < paths.txt');
  process.exit(2);
}
const WIDTH = 800, HEIGHT = 600, PAGES = 4, TIMEOUT_MS = 10000;
const paths = (await readStdin()).split('\n').map((l) => l.trim()).filter(Boolean);

const TYPES = {
  '.html': 'text/html', '.htm': 'text/html', '.xht': 'application/xhtml+xml',
  '.xhtml': 'application/xhtml+xml', '.xml': 'application/xml', '.css': 'text/css',
  '.js': 'text/javascript', '.png': 'image/png', '.jpg': 'image/jpeg', '.gif': 'image/gif',
  '.svg': 'image/svg+xml', '.ttf': 'font/ttf', '.otf': 'font/otf', '.woff': 'font/woff',
  '.woff2': 'font/woff2', '.webp': 'image/webp',
};
const server = createServer(async (req, res) => {
  const path = normalize(join(wptDir, decodeURIComponent(new URL(req.url, 'http://x').pathname)));
  try {
    if (!path.startsWith(wptDir)) throw new Error('outside WPT');
    const body = await readFile(path);
    res.writeHead(200, { 'content-type': TYPES[extname(path)] ?? 'application/octet-stream' });
    res.end(body);
  } catch {
    res.writeHead(404);
    res.end();
  }
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const origin = `http://127.0.0.1:${server.address().port}`;

const tmp = await mkdtemp(join(tmpdir(), 'xerune-chromium-'));
await mkdir(join(tmp, 'fonts'));
await writeFile(join(tmp, 'fonts', 'Ahem.ttf'), await readFile(join(wptDir, 'fonts', 'Ahem.ttf')));
await writeFile(join(tmp, 'fonts.conf'), `<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <dir>${tmp}/fonts</dir>
  <cachedir>${tmp}/fontcache</cachedir>
  <match target="pattern"><edit name="family" mode="assign" binding="strong"><string>Ahem</string></edit></match>
  <match target="font"><edit name="embolden" mode="assign"><bool>false</bool></edit></match>
</fontconfig>
`);

const chromium = spawn(process.env.CHROMIUM ?? 'chromium', [
  '--headless', '--no-sandbox', '--disable-gpu', '--hide-scrollbars', '--mute-audio',
  '--force-device-scale-factor=1', '--font-render-hinting=none', '--disable-lcd-text',
  '--disable-renderer-backgrounding', '--disable-background-timer-throttling',
  '--disable-backgrounding-occluded-windows', `--window-size=${WIDTH},${HEIGHT}`,
  '--remote-debugging-port=0', `--user-data-dir=${tmp}/profile`, 'about:blank',
], { env: { ...process.env, FONTCONFIG_FILE: join(tmp, 'fonts.conf') }, stdio: ['ignore', 'ignore', 'pipe'] });
const wsUrl = await new Promise((ok, fail) => {
  let log = '';
  chromium.stderr.on('data', (d) => {
    log += d;
    const m = log.match(/DevTools listening on (ws:\S+)/);
    if (m) ok(m[1]);
  });
  chromium.on('exit', (code) => fail(new Error(`chromium exited (${code}): ${log}`)));
});

const ws = new WebSocket(wsUrl);
await new Promise((r) => ws.addEventListener('open', r, { once: true }));
let nextId = 0;
const pending = new Map(), listeners = new Set();
ws.addEventListener('message', ({ data }) => {
  const msg = JSON.parse(data);
  if (msg.id !== undefined) {
    const p = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) p.fail(new Error(msg.error.message)); else p.ok(msg.result);
  } else {
    for (const l of listeners) l(msg);
  }
});
const send = (method, params = {}, sessionId) => new Promise((ok, fail) => {
  const id = nextId++;
  pending.set(id, { ok, fail });
  ws.send(JSON.stringify({ id, method, params, sessionId }));
});
const event = (sessionId, method) => new Promise((ok) => {
  const l = (msg) => {
    if (msg.sessionId === sessionId && msg.method === method) { listeners.delete(l); ok(msg.params); }
  };
  listeners.add(l);
});
const withTimeout = (promise) => Promise.race([
  promise, new Promise((_, fail) => setTimeout(() => fail(new Error('timeout')), TIMEOUT_MS)),
]);

async function openPage() {
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  await send('Page.enable', {}, sessionId);
  // Only the front tab gets animation frames otherwise.
  await send('Emulation.setFocusEmulationEnabled', { enabled: true }, sessionId);
  await send('Emulation.setDeviceMetricsOverride',
    { width: WIDTH, height: HEIGHT, deviceScaleFactor: 1, mobile: false }, sessionId);
  return { targetId, sessionId };
}

async function capture(page, path) {
  const loaded = event(page.sessionId, 'Page.loadEventFired');
  await send('Page.navigate', { url: `${origin}/${path}` }, page.sessionId);
  await loaded;
  // Same settling as the WPT reftest harness: fonts, then two frames.
  await send('Runtime.evaluate', {
    expression: `document.fonts.ready.then(() => new Promise(r =>
      requestAnimationFrame(() => requestAnimationFrame(r))))`,
    awaitPromise: true,
  }, page.sessionId);
  const { data } = await send('Page.captureScreenshot',
    { format: 'png', clip: { x: 0, y: 0, width: WIDTH, height: HEIGHT, scale: 1 } }, page.sessionId);
  const out = join(outDir, `${path}.png`);
  await mkdir(dirname(out), { recursive: true });
  await writeFile(out, Buffer.from(data, 'base64'));
}

let done = 0, failed = 0;
const queue = paths.filter((p) => existsSync(join(wptDir, p)));
async function worker() {
  let page = await openPage();
  for (let path = queue.shift(); path !== undefined; path = queue.shift()) {
    try {
      await withTimeout(capture(page, path));
    } catch (e) {
      failed++;
      console.error(`chromium: ${path}: ${e.message}`);
      // A page stuck in a timeout may never fire events again.
      await send('Target.closeTarget', { targetId: page.targetId }).catch(() => {});
      page = await openPage();
    }
    if (++done % 500 === 0) console.error(`chromium: ${done}/${paths.length}`);
  }
}
await Promise.all(Array.from({ length: PAGES }, worker));
console.error(`chromium: ${done - failed} screenshots, ${failed} failed`);

const exited = new Promise((r) => chromium.once('exit', r));
await send('Browser.close').catch(() => {});
await exited;
server.close();
await rm(tmp, { recursive: true, force: true });
process.exit(0);

async function readStdin() {
  let s = '';
  for await (const chunk of process.stdin) s += chunk;
  return s;
}
