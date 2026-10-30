// Packs @auditaur/api, installs the tarball into a scratch project, and imports it with plain
// Node, the way vitest's importActual and Node test helpers load it. Bundlers forgive
// extensionless relative imports; Node does not (AR-19).
import { execFileSync, execSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const pkg = resolve(fileURLToPath(new URL('..', import.meta.url)));
const scratch = mkdtempSync(join(tmpdir(), 'auditaur-api-smoke-'));
// Prefer the npm running this script (`npm run` sets npm_execpath); otherwise Windows runs npm
// through its .cmd shim, which needs a shell.
const npmCli = process.env.npm_execpath?.endsWith('.js') ? process.env.npm_execpath : null;
const npm = (args, cwd) =>
  npmCli
    ? execFileSync(process.execPath, [npmCli, ...args], { cwd, encoding: 'utf8' })
    : process.platform === 'win32'
      ? execSync(['npm', ...args.map((arg) => `"${arg}"`)].join(' '), { cwd, encoding: 'utf8' })
      : execFileSync('npm', args, { cwd, encoding: 'utf8' });

try {
  const tarball = JSON.parse(npm(['pack', '--json', '--pack-destination', scratch], pkg))[0].filename;
  writeFileSync(join(scratch, 'package.json'), JSON.stringify({ name: 'smoke', private: true, type: 'module' }));
  npm(['install', '--no-audit', '--no-fund', '--silent', join(scratch, tarball)], scratch);
  writeFileSync(
    join(scratch, 'smoke.mjs'),
    `import * as api from '@auditaur/api';
const required = ['initAuditaur', 'recordedPayload', 'recordedErrorMessage', 'REDACTED'];
const missing = required.filter((name) => !(name in api));
if (missing.length) throw new Error('missing exports: ' + missing.join(', '));
const recorded = api.recordedPayload({ redactKeys: ['email'] }, 'invokeResult', 'list', { email: 'a@b.c' });
if (JSON.stringify(recorded).includes('a@b.c')) throw new Error('recordedPayload did not redact');
console.log('@auditaur/api imports under Node: ' + Object.keys(api).length + ' exports');
`,
  );
  execFileSync(process.execPath, ['smoke.mjs'], { cwd: scratch, stdio: 'inherit' });
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
