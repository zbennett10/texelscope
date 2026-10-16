// A headless Chrome with a profile of its own, driven over the DevTools protocol, and a Vite
// server for the pages it loads. Tests that need a GPU skip when no Chrome is installed.

import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer, type ViteDevServer } from 'vite';

const CANDIDATES = [
  process.env.CHROME_PATH,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
];

/** The Chrome to run, if one is installed. */
export const chromePath = CANDIDATES.find((p): p is string => !!p && existsSync(p));

export interface Browser {
  /** Evaluates an expression in the page and returns its value, awaiting a promise. */
  evaluate<T>(expression: string): Promise<T>;
  close(): Promise<void>;
}

/** Serves `page` from this folder and opens it in a new headless Chrome. */
export async function openPage(page: string): Promise<Browser> {
  const root = fileURLToPath(new URL('.', import.meta.url));
  const server: ViteDevServer = await createServer({
    root,
    logLevel: 'silent',
    server: { host: '127.0.0.1', port: 0, strictPort: false, fs: { allow: [join(root, '..', '..')] } },
  });
  await server.listen();
  const url = new URL(page, server.resolvedUrls!.local[0]!).href;
  const profile = mkdtempSync(join(tmpdir(), 'texelscope-chrome-'));
  const chrome: ChildProcess = spawn(
    chromePath!,
    ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check', '--disable-popup-blocking', '--window-size=800,600', 'about:blank'],
    { stdio: 'ignore' },
  );
  const port = await debugPort(profile);
  const target = await (await fetch(`http://127.0.0.1:${port}/json/new?${encodeURIComponent(url)}`, { method: 'PUT' })).json() as { webSocketDebuggerUrl: string };
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.onopen = resolve;
    socket.onerror = reject;
  });
  let next = 0;
  const pending = new Map<number, (message: { result?: unknown; error?: { message: string } }) => void>();
  socket.onmessage = (event) => {
    const message = JSON.parse(String(event.data));
    pending.get(message.id)?.(message);
    pending.delete(message.id);
  };
  const send = (method: string, params: object) =>
    new Promise<{ result?: unknown; error?: { message: string } }>((resolve) => {
      const id = ++next;
      pending.set(id, resolve);
      socket.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async <T>(expression: string): Promise<T> => {
    const reply = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (reply.error) throw new Error(reply.error.message);
    const result = reply.result as { result: { value: T }; exceptionDetails?: { exception?: { description?: string }; text: string } };
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
    return result.result.value;
  };
  // The page module loads after navigation, and says so on the window.
  for (let i = 0; i < 200 && !(await evaluate<boolean>('!!window.ready')); i++) await new Promise((r) => setTimeout(r, 50));
  return {
    evaluate,
    async close() {
      socket.close();
      chrome.kill();
      await new Promise((r) => chrome.once('exit', r));
      rmSync(profile, { recursive: true, force: true });
      await server.close();
    },
  };
}

/** The port Chrome wrote to its profile once it listens. */
async function debugPort(profile: string): Promise<number> {
  const file = join(profile, 'DevToolsActivePort');
  for (let i = 0; i < 200; i++) {
    if (existsSync(file)) {
      const port = Number(readFileSync(file, 'utf8').split('\n')[0]);
      if (port) return port;
    }
    await new Promise((r) => setTimeout(r, 50));
  }
  throw new Error('Chrome did not start');
}
