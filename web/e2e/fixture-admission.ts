import childProcess from 'node:child_process';
import { isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';

type Operation = 'worker' | 'begin' | 'finish';
const helper = fileURLToPath(new URL('../../scripts/browser_order_fixtures.py', import.meta.url));
const invalid = () => new Error('Browser fixture admission failed; details withheld.');

export async function admission(operation: Operation, generation: number, value = 0, timeout = 60_000): Promise<number> {
  const path = process.env.OPENTRACK_BROWSER_ADMISSION_FILE;
  if (!path || !isAbsolute(path) || !['worker', 'begin', 'finish'].includes(operation)
      || !Number.isInteger(generation) || generation < 0 || generation > 1_000_000
      || !Number.isInteger(timeout) || timeout <= 0 || timeout > 60_000
      || !Number.isInteger(value) || value < (operation === 'worker' ? 0 : 1)
      || value > (operation === 'worker' ? 0 : operation === 'begin' ? 20 : 1_000_000)) throw invalid();
  const env = Object.fromEntries(['PATH', 'LANG', 'LC_ALL', 'TZ'].flatMap(key =>
    process.env[key] === undefined ? [] : [[key, process.env[key]!]]));
  return new Promise((resolve, reject) => {
    childProcess.execFile('python3', ['-I', '-B', helper, '--admission', operation, path,
      String(generation), String(value), String(timeout)], { env, timeout, maxBuffer: 1024, windowsHide: true }, (error, stdout) => {
      if (error) { reject(invalid()); return; }
      const lease = /^\s*\{\s*"lease"\s*:\s*([1-9][0-9]{0,6})\s*\}\s*$/.exec(stdout);
      if (operation === 'begin' && lease && Number(lease[1]) <= 1_000_000) resolve(Number(lease[1]));
      else if (operation !== 'begin' && /^\s*\{\s*\}\s*$/.test(stdout)) resolve(0);
      else reject(invalid());
    });
  });
}
