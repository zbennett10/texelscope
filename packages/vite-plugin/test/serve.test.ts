import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { afterAll, describe, expect, it, vi } from 'vitest';

// A file that goes away between the check and the read, as when the UI is built again.
vi.mock('node:fs', async (load) => {
  const fs = await load<typeof import('node:fs')>();
  return {
    ...fs,
    createReadStream: (path: string) =>
      path.endsWith('gone.js')
        ? new Readable({ read() { this.destroy(Object.assign(new Error('ENOENT: no such file'), { code: 'ENOENT' })); } })
        : fs.createReadStream(path),
  };
});

const dir = mkdtempSync(join(tmpdir(), 'texelscope-ui-'));
afterAll(() => rmSync(dir, { recursive: true, force: true }));

describe('the UI the plugin serves', () => {
  it('answers 404 for a file that goes away while it is read, and keeps serving', async () => {
    writeFileSync(join(dir, 'index.html'), '<!doctype html>');
    writeFileSync(join(dir, 'gone.js'), 'x');
    const { default: texelscope } = await import('../src/index.js');
    let serve: ((req: unknown, res: unknown, next: () => void) => void) | undefined;
    const hook = texelscope({ uiDir: dir }).configureServer as (server: unknown) => void;
    hook({ middlewares: { use: (_base: string, handler: typeof serve) => (serve = handler) } });
    const server = createServer((req, res) => serve!(req, res, () => res.end()));
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
    const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    try {
      expect((await fetch(`${url}/gone.js`)).status).toBe(404);
      expect((await fetch(`${url}/`)).status).toBe(200);
    } finally {
      server.close();
    }
  });
});
