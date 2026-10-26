import test, { afterEach, beforeEach, mock } from 'node:test';
import assert from 'node:assert/strict';
import childProcess from 'node:child_process';
import { admission } from './fixture-admission.ts';

const marker = 'SYNTHETIC_PRIVATE_MARKER';
const keys = ['OPENTRACK_BROWSER_ADMISSION_FILE', 'OPENTRACK_BROWSER_PASSWORD', 'OPENTRACK_BROWSER_SETUP_TOKEN', 'PGPASSWORD', 'NODE_OPTIONS'];
let original;
beforeEach(() => {
  original = Object.fromEntries(keys.map(key => [key, process.env[key]]));
  process.env.OPENTRACK_BROWSER_ADMISSION_FILE = process.platform === 'win32' ? 'C:\\fixture\\admission.json' : '/fixture/admission.json';
});
afterEach(() => {
  mock.restoreAll();
  for (const key of keys) {
    if (original[key] === undefined) delete process.env[key];
    else process.env[key] = original[key];
  }
});
function execute(stdout = '{}', error = null) {
  return mock.method(childProcess, 'execFile', (_command, _args, _options, callback) => {
    queueMicrotask(() => callback(error, stdout, marker));
  });
}
const withheld = error => error.message === 'Browser fixture admission failed; details withheld.';

test('uses a fixed direct command and passes only the private data path and bounded numeric facts', async () => {
  const command = execute('{"lease":7}');
  assert.equal(await admission('begin', 3, 6, 1234), 7);
  const [program, args, options] = command.mock.calls[0].arguments;
  assert.equal(program, 'python3');
  assert.deepEqual(args.slice(0, 2), ['-I', '-B']);
  assert.equal(args[2].endsWith('browser_order_fixtures.py'), true);
  assert.equal(args[3], '--admission');
  assert.deepEqual(args.slice(4), ['begin', process.env.OPENTRACK_BROWSER_ADMISSION_FILE, '3', '6', '1234']);
  assert.equal(options.timeout, 1234);
  assert.equal(options.maxBuffer, 1024);
  assert.equal(options.windowsHide, true);
  assert.equal(options.shell, undefined);
});

test('coordinator children receive no credentials or command-control environment', async () => {
  for (const key of keys.slice(1)) process.env[key] = marker;
  const command = execute();
  assert.equal(await admission('worker', 9), 0);
  const env = command.mock.calls[0].arguments[2].env;
  assert.equal(Object.values(env).includes(marker), false);
  assert.equal(Object.keys(env).every(key => ['PATH', 'LANG', 'LC_ALL', 'TZ'].includes(key)), true);
});

test('rejects missing or relative state without a subprocess or fallback', async () => {
  const command = execute();
  for (const path of [undefined, '', 'relative.json']) {
    if (path === undefined) delete process.env.OPENTRACK_BROWSER_ADMISSION_FILE;
    else process.env.OPENTRACK_BROWSER_ADMISSION_FILE = path;
    await assert.rejects(admission('worker', 0), withheld);
  }
  assert.equal(command.mock.callCount(), 0);
});

test('rejects invalid phases, generations, leases and timeout extensions before invocation', async () => {
  const command = execute();
  for (const args of [['begin', 0, 21], ['begin', 0, 0], ['worker', -1], ['worker', 0, 1],
    ['worker', 0.5], ['finish', 0, 0], ['finish', 0, 1_000_001], ['begin', 0, 1, 60_001],
    ['begin', 0, 1, 0], ['begin', 0, 1, NaN], ['unknown', 0]]) {
    await assert.rejects(admission(...args), withheld);
  }
  assert.equal(command.mock.callCount(), 0);
});

test('accepts only exact bounded lease results and never echoes poisoned stdout', async () => {
  for (const stdout of ['null', '[]', '{}', '{"lease":0}', '{"lease":1.5}', '{"lease":true}',
    '{"lease":1000001}', '{"lease":1,"lease":2}', JSON.stringify({ lease: 1, secret: marker }), marker]) {
    const command = execute(stdout);
    await assert.rejects(admission('begin', 0, 1), withheld);
    command.mock.restore();
  }
});

test('worker and completion results cannot carry state, clocks or additional fields', async () => {
  for (const operation of ['worker', 'finish']) {
    const command = execute('{"deadline_ns":123}');
    await assert.rejects(admission(operation, 1, operation === 'finish' ? 1 : 0), withheld);
    command.mock.restore();
  }
  execute();
  assert.equal(await admission('finish', 1, 8), 0);
});

test('timeout and child errors remain failures without retry or raw diagnostic exposure', async () => {
  const command = execute(marker, Object.assign(new Error(marker), { code: 'ETIMEDOUT', stderr: marker }));
  await assert.rejects(admission('begin', 0, 1), withheld);
  assert.equal(command.mock.callCount(), 1);
});
