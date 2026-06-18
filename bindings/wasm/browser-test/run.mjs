import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve, extname } from 'node:path';
import { readFile } from 'node:fs/promises';

const root = resolve(import.meta.dirname, '..');
const testRoot = resolve(import.meta.dirname);
const chrome = process.env.CHROME || '/Applications/Chromium.app/Contents/MacOS/Chromium';

const contentType = (path) => {
  switch (extname(path)) {
    case '.html':
      return 'text/html; charset=utf-8';
    case '.js':
      return 'text/javascript; charset=utf-8';
    case '.wasm':
      return 'application/wasm';
    default:
      return 'application/octet-stream';
  }
};

const server = createServer(async (req, res) => {
  if (req.method === 'POST' && req.url === '/__loom_done__') {
    let body = '';
    req.setEncoding('utf8');
    req.on('data', (chunk) => {
      body += chunk;
    });
    req.on('end', () => {
      res.writeHead(204).end();
      server.emit('loom-result', JSON.parse(body));
    });
    return;
  }

  const url = new URL(req.url || '/', 'http://127.0.0.1');
  const pathname = url.pathname === '/' ? '/browser-test/index.html' : url.pathname;
  const base = pathname.startsWith('/pkg/') ? root : testRoot;
  const filePath = pathname.startsWith('/pkg/')
    ? join(base, pathname.slice(1))
    : join(base, pathname.replace(/^\/browser-test\//, ''));
  try {
    const bytes = await readFile(filePath);
    res.writeHead(200, {
      'content-type': contentType(filePath),
      'cross-origin-opener-policy': 'same-origin',
      'cross-origin-embedder-policy': 'require-corp',
    });
    res.end(bytes);
  } catch {
    res.writeHead(404).end('not found');
  }
});

const waitForResult = () =>
  new Promise((resolveResult, reject) => {
    const timer = setTimeout(() => reject(new Error('browser runtime timed out')), 30000);
    server.once('loom-result', (result) => {
      clearTimeout(timer);
      resolveResult(result);
    });
  });

const waitForExit = (child) =>
  new Promise((resolveExit) => {
    child.once('exit', resolveExit);
  });

const userDataDir = await mkdtemp(join(tmpdir(), 'loom-wasm-browser-'));
try {
  await new Promise((resolveListen) => server.listen(0, '127.0.0.1', resolveListen));
  const port = server.address().port;
  const browser = spawn(chrome, [
    '--headless=new',
    '--disable-gpu',
    `--user-data-dir=${userDataDir}`,
    `http://127.0.0.1:${port}/browser-test/index.html`,
  ], { stdio: 'ignore' });

  const result = await waitForResult();
  const exited = waitForExit(browser);
  browser.kill('SIGTERM');
  await exited;
  if (!result.ok) {
    throw new Error(result.error || 'browser runtime failed');
  }
  console.log('wasm browser runtime passed');
} finally {
  server.close();
  await rm(userDataDir, { recursive: true, force: true });
}
