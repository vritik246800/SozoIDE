// The built site checked (TODO 6.4.7, 6.F.2), with nothing but Node and the Chrome the runner
// already has: every internal link and file exists, nothing is asked of another domain than
// GitHub, links that open a new tab have rel="noopener", and at 320, 390 and 1440 px no page
// scrolls sideways nor writes an error to the console.
//
//   node scripts/check-site.mjs            (after `npm run build`; CHROME=… to pick Chrome)
import { createServer } from 'node:http';
import { readFileSync, existsSync, statSync, readdirSync } from 'node:fs';
import { join, extname, resolve } from 'node:path';
import { spawn, spawnSync } from 'node:child_process';

const dist = resolve(import.meta.dirname, '../dist');
const base = '/SozoIDE';
const failures = [];
const fail = (message) => failures.push(message);

// --- What the HTML says ---------------------------------------------------------------------
const pages = [];
const walk = (folder) => {
  for (const entry of readdirSync(folder)) {
    const path = join(folder, entry);
    if (statSync(path).isDirectory()) walk(path);
    else if (entry.endsWith('.html')) pages.push(path);
  }
};
walk(dist);
for (const page of pages) {
  const html = readFileSync(page, 'utf8');
  const name = page.slice(dist.length) || '/';
  for (const [, attribute, value] of html.matchAll(/\b(href|src|content)="([^"]+)"/g)) {
    if (attribute === 'content' && !/^https?:/.test(value)) continue;
    if (/^(mailto:|#|data:)/.test(value)) continue;
    if (/^https?:\/\//.test(value)) {
      const host = new URL(value).hostname;
      const allowed = ['github.com', 'vritik246800.github.io', 'schema.org', 'www.apache.org', 'www.sitemaps.org'];
      if (!allowed.includes(host) && !host.endsWith('.vercel.app')) fail(`${name}: asks ${host} (${value})`);
      continue;
    }
    if (!value.startsWith(base)) {
      fail(`${name}: a link outside ${base}: ${value}`);
      continue;
    }
    const path = decodeURIComponent(value.slice(base.length).split(/[?#]/)[0]);
    const file = join(dist, path, path.endsWith('/') || !extname(path) ? 'index.html' : '');
    if (!existsSync(file) && !existsSync(join(dist, path))) fail(`${name}: ${value} does not exist`);
  }
  for (const [tag] of html.matchAll(/<a\b[^>]*target="_blank"[^>]*>/g)) {
    if (!/rel="[^"]*noopener/.test(tag)) fail(`${name}: ${tag} has no rel="noopener"`);
  }
}

// --- What the Vercel upload has ------------------------------------------------------------
// The .vercelignore is an allowlist: a file outside site/ that a page reads must pass it, or the
// Vercel build stops with ENOENT while CI and Pages, with the whole repository, see nothing (E87).
const repository = resolve(import.meta.dirname, '../..');
const sources = [];
const walkSources = (folder) => {
  for (const entry of readdirSync(folder)) {
    const path = join(folder, entry);
    if (statSync(path).isDirectory()) walkSources(path);
    else if (/\.(astro|ts|mjs)$/.test(entry)) sources.push(path);
  }
};
walkSources(resolve(import.meta.dirname, '../src'));
for (const source of sources) {
  for (const [, file] of readFileSync(source, 'utf8').matchAll(/process\.cwd\(\), '\.\.\/([^']+)'/g)) {
    const ignored = spawnSync('git', ['-c', `core.excludesFile=${join(repository, '.vercelignore')}`, 'check-ignore', '--no-index', '-q', '--', file], { cwd: repository });
    if (ignored.status === 0) fail(`${source.slice(repository.length + 1)} reads ${file}, which .vercelignore leaves out of the Vercel upload`);
    else if (ignored.status !== 1) fail(`git check-ignore could not say whether Vercel gets ${file}`);
  }
}

// --- What a browser sees ----------------------------------------------------------------------
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.woff2': 'font/woff2', '.xml': 'application/xml', '.txt': 'text/plain' };
const server = createServer((request, response) => {
  let path = decodeURIComponent(new URL(request.url, 'http://x').pathname);
  if (!path.startsWith(base)) return response.writeHead(404).end();
  path = path.slice(base.length) || '/';
  let file = join(dist, path);
  if (existsSync(file) && statSync(file).isDirectory()) file = join(file, 'index.html');
  if (!file.startsWith(dist) || !existsSync(file)) return response.writeHead(404).end();
  response.writeHead(200, { 'Content-Type': types[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((done) => server.listen(0, '127.0.0.1', done));
const origin = `http://127.0.0.1:${server.address().port}`;

const chrome = process.env.CHROME
  ?? ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable', '/usr/bin/chromium']
    .find((path) => existsSync(path));
if (!chrome) {
  console.error('No Chrome found: set CHROME to its path');
  process.exit(1);
}
// The runners of GitHub (Ubuntu 26) refuse the user namespaces of Chrome's sandbox; there it
// opens only this site, served from this machine (ERRORS.md E86).
const sandbox = process.env.CI && process.platform === 'linux' ? ['--no-sandbox'] : [];
const browser = spawn(chrome, ['--headless=new', ...sandbox, '--remote-debugging-port=0', '--no-first-run', '--no-default-browser-check', `--user-data-dir=${resolve(dist, '../.chrome-check')}`, 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
const endpoint = await new Promise((done, failed) => {
  let text = '';
  browser.stderr.on('data', (chunk) => {
    text += chunk;
    const found = text.match(/ws:\/\/[^\s]+/);
    if (found) done(found[0]);
  });
  browser.on('exit', (code) => failed(new Error(`Chrome stopped (${code}):\n${text.slice(-2000)}`)));
  setTimeout(() => failed(new Error(`Chrome did not start:\n${text.slice(-2000)}`)), 20000);
});
const version = await (await fetch(endpoint.replace('ws://', 'http://').replace(/\/devtools\/browser\/.*/, '/json/version'))).json();
const socket = new WebSocket(version.webSocketDebuggerUrl);
await new Promise((done) => socket.addEventListener('open', done));
let next = 0;
const waiting = new Map();
const events = [];
socket.addEventListener('message', ({ data }) => {
  const message = JSON.parse(data);
  if (message.id && waiting.has(message.id)) {
    waiting.get(message.id)(message);
    waiting.delete(message.id);
  } else if (message.method) events.push(message);
});
const send = (method, params = {}, sessionId) =>
  new Promise((done) => {
    const id = ++next;
    waiting.set(id, done);
    socket.send(JSON.stringify({ id, method, params, sessionId }));
  });

const { result: { targetId } } = await send('Target.createTarget', { url: 'about:blank' });
const { result: { sessionId } } = await send('Target.attachToTarget', { targetId, flatten: true });
await send('Runtime.enable', {}, sessionId);
await send('Log.enable', {}, sessionId);
await send('Page.enable', {}, sessionId);
const routes = ['/', '/install/', '/notes/', '/shortcuts/'];
for (const width of [320, 390, 1440]) {
  await send('Emulation.setDeviceMetricsOverride', { width, height: 800, deviceScaleFactor: 1, mobile: width < 600 }, sessionId);
  for (const route of routes) {
    events.length = 0;
    await send('Page.navigate', { url: `${origin}${base}${route}` }, sessionId);
    await new Promise((done) => setTimeout(done, 1500));
    const { result } = await send('Runtime.evaluate', { expression: 'document.documentElement.scrollWidth - window.innerWidth', returnByValue: true }, sessionId);
    const extra = result.result.value;
    if (extra > 0) fail(`${route} at ${width} px scrolls sideways by ${extra} px`);
    for (const event of events) {
      const error = event.method === 'Runtime.exceptionThrown'
        || (event.method === 'Log.entryAdded' && event.params.entry.level === 'error')
        || (event.method === 'Runtime.consoleAPICalled' && event.params.type === 'error');
      if (error) fail(`${route} at ${width} px: ${JSON.stringify(event.params).slice(0, 200)}`);
    }
  }
}
socket.close();
browser.kill();
server.close();

if (failures.length > 0) {
  console.error(failures.join('\n'));
  process.exit(1);
}
console.log(`Site checked: ${pages.length} pages, links, domains, 320/390/1440 px, console`);
