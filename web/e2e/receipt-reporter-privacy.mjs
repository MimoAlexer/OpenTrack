import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import ReceiptReporter, { parseSavedReadiness } from './receipt-reporter.ts';

const marker = 'SYNTHETIC_PRIVATE_MARKER_MUST_NOT_BE_PUBLISHED';
const sample = () => ({
  setup_status: { state: 'completed', status: 200, elapsed_ms: 15 },
  session: { state: 'failed', status: null, elapsed_ms: 24 },
  bootstrap: { state: 'pending', status: 200, elapsed_ms: 10000 },
  access: { state: 'unobserved', status: null, elapsed_ms: 0 },
});
const parse = value => parseSavedReadiness(JSON.stringify(value));

test('accepts only the fixed bounded request-state schema', () => {
  assert.deepEqual(parse(sample()), sample());
  for (const state of ['pending', 'completed', 'failed']) {
    const value = sample();
    value.access = { state, status: 599, elapsed_ms: 120000 };
    assert.deepEqual(parse(value), value);
  }
});

test('rejects unknown keys, missing routes and unexpected types at every level', () => {
  const changes = [
    value => { value.headers = marker; }, value => { delete value.access; },
    value => { value.bootstrap.url = marker; }, value => { delete value.bootstrap.status; },
    value => { value.bootstrap = []; }, value => { value.bootstrap = null; },
    value => { value.bootstrap.state = ['pending']; }, value => { value.bootstrap.state = 1; },
    value => { value.bootstrap.status = '200'; }, value => { value.bootstrap.status = true; },
    value => { value.bootstrap.elapsed_ms = '12'; }, value => { value.bootstrap.elapsed_ms = null; },
  ];
  for (const change of changes) {
    const value = sample(); change(value); assert.equal(parse(value), undefined);
  }
  for (const value of [null, [], true, 1, marker]) assert.equal(parse(value), undefined);
});

test('rejects out-of-bounds and inconsistent observations', () => {
  for (const [field, values] of [['status', [0, 99, 600, 200.5]], ['elapsed_ms', [-1, 120001, 1.5, Infinity]]]) {
    for (const invalid of values) {
      const value = sample(); value.bootstrap[field] = invalid; assert.equal(parse(value), undefined);
    }
  }
  for (const change of [value => { value.access.status = 200; }, value => { value.access.elapsed_ms = 1; },
    value => { value.setup_status.status = null; }]) {
    const value = sample(); change(value); assert.equal(parse(value), undefined);
  }
});

test('rejects injected marker strings, oversized encodings, malformed JSON and prototype keys', () => {
  for (const field of ['state', 'status', 'elapsed_ms']) {
    const value = sample(); value.bootstrap[field] = marker; assert.equal(parse(value), undefined);
  }
  for (const encoded of [marker, '{', ' '.repeat(2049), JSON.stringify({ ...sample(), body: marker.repeat(100) }),
    JSON.stringify(sample()).replace('"setup_status":', '"__proto__":{},"setup_status":')]) {
    assert.equal(parseSavedReadiness(encoded), undefined);
  }
  for (const value of [undefined, null, 200, {}]) assert.equal(parseSavedReadiness(value), undefined);
});

test('publishes only one validated failure annotation without raw output or errors', () => {
  const directory = mkdtempSync(join(tmpdir(), 'opentrack-readiness-'));
  const previousDirectory = process.cwd();
  const output = [];
  const log = console.log, error = console.error;
  try {
    mkdirSync(join(directory, 'run'));
    process.chdir(join(directory, 'run'));
    console.log = console.error = value => output.push(String(value));
    const good = { type: 'saved-readiness', description: JSON.stringify(sample()) };
    const annotations = [[good], [{ type: 'other', description: marker }],
      [{ type: 'saved-readiness', description: JSON.stringify({ ...sample(), cookie: marker }) }],
      [good, good], [{ type: 'saved-readiness', description: marker }]];
    const reporter = new ReceiptReporter();
    const testCase = { title: 'Synthetic readiness test', parent: { project: () => ({ name: 'chromium' }) },
      location: { file: 'fixtures.ts', line: 32 } };
    for (const annotation of annotations) {
      reporter.onTestEnd(testCase, { annotations: annotation, status: 'failed', duration: 10,
        error: { message: marker, stack: `${marker}\n at fixtures.ts:32:1` },
        stdout: [marker], stderr: [marker], body: marker, cookies: marker });
    }
    reporter.onTestEnd(testCase, { annotations: [good], status: 'passed', duration: 10 });
    reporter.onError(new Error(marker));
    reporter.onEnd({ status: 'failed' });
    const encoded = readFileSync(join(directory, 'work/browser-acceptance/receipt.json'), 'utf8');
    const receipt = JSON.parse(encoded);
    assert.deepEqual(receipt.checks[0].saved_readiness, sample());
    assert.ok(receipt.checks.slice(1).every(check => !Object.hasOwn(check, 'saved_readiness')));
    assert.ok(!encoded.includes(marker));
    assert.ok(!output.join('\n').includes(marker));
    assert.equal(receipt.checks[0].failure, 'fixtures.ts:32');
  } finally {
    console.log = log; console.error = error; process.chdir(previousDirectory);
    assert.equal(dirname(resolve(directory)), resolve(tmpdir()));
    assert.ok(basename(directory).startsWith('opentrack-readiness-'));
    rmSync(directory, { recursive: true, force: true });
  }
});
