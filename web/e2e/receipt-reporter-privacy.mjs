import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import ReceiptReporter, { parseProjectLeadReadiness, parseSavedReadiness } from './receipt-reporter.ts';

const marker = 'SYNTHETIC_PRIVATE_MARKER_MUST_NOT_BE_PUBLISHED';
const sample = () => ({
  setup_status: { state: 'completed', status: 200, elapsed_ms: 15 },
  session: { state: 'failed', status: null, elapsed_ms: 24 },
  bootstrap: { state: 'pending', status: 200, elapsed_ms: 10000 },
  access: { state: 'unobserved', status: null, elapsed_ms: 0 },
});
const parse = value => parseSavedReadiness(JSON.stringify(value));
const leadSample = () => ({
  request_state: 'completed', status: 200, elapsed_ms: 23, q_scalar_length: 0,
  returned_count: 20, has_more: true, self_assignable: true, fixture_owner_in_page: false,
  expected_display_name_match: true, current_user_id_match: true,
  option_observation: 'before_click', owner_option_match_count: 1, you_marker_option_count: 1,
  owner_option_visible: true, owner_option_enabled: true,
  click_failure: null, center_hit_self_or_descendant: null, center_inside_dialog: null,
  center_inside_viewport: null, matching_dialog_count: null,
  option_rect: null, popover_rect: null, wrapper_rect: null, trigger_rect: null, dialog_rect: null,
  center_hit_another_option: null, owner_option_content_rect: null, hit_option_rect: null, hit_option_content_rect: null,
});
const parseLead = value => parseProjectLeadReadiness(JSON.stringify(value));

test('accepts bounded project lead facts without requiring the owner in the returned page', () => {
  assert.deepEqual(parseLead(leadSample()), leadSample());
  for (const state of ['pending', 'completed', 'failed']) {
    const value = leadSample(); value.request_state = state; value.elapsed_ms = 120000;
    assert.deepEqual(parseLead(value), value);
  }
  const unknown = leadSample();
  Object.assign(unknown, { request_state: 'unobserved', status: null, elapsed_ms: 0,
    q_scalar_length: null, returned_count: null, has_more: null, self_assignable: null,
    fixture_owner_in_page: null, option_observation: 'unobserved', owner_option_match_count: null,
    you_marker_option_count: null, owner_option_visible: null, owner_option_enabled: null });
  assert.deepEqual(parseLead(unknown), unknown);
});

test('rejects extra, missing, nested and secret-bearing project lead fields', () => {
  for (const key of Object.keys(leadSample())) {
    const missing = leadSample(); delete missing[key]; assert.equal(parseLead(missing), undefined);
    const injected = leadSample(); injected[key] = marker; assert.equal(parseLead(injected), undefined);
    const nested = leadSample(); nested[key] = { body: marker }; assert.equal(parseLead(nested), undefined);
  }
  for (const key of ['headers', 'body', 'url', 'cookie', 'name', 'id', '__proto__']) {
    const encoded = JSON.stringify(leadSample()).replace('{', `{${JSON.stringify(key)}:${JSON.stringify(marker)},`);
    assert.equal(parseProjectLeadReadiness(encoded), undefined);
  }
  for (const value of [null, [], true, 1, marker]) assert.equal(parseLead(value), undefined);
  for (const value of [undefined, null, {}, 200, '{', marker, ' '.repeat(2049)]) {
    assert.equal(parseProjectLeadReadiness(value), undefined);
  }
});

test('rejects project lead values outside the exact scalar and observation bounds', () => {
  for (const [key, invalid] of [
    ['status', 99], ['status', 600], ['status', 200.5], ['elapsed_ms', -1], ['elapsed_ms', 120001],
    ['q_scalar_length', -1], ['q_scalar_length', 101], ['q_scalar_length', 0.5],
    ['returned_count', -1], ['returned_count', 21], ['returned_count', 1.5],
    ['owner_option_match_count', -1], ['owner_option_match_count', 23],
    ['you_marker_option_count', -1], ['you_marker_option_count', 2],
    ['request_state', 'unknown'], ['option_observation', 'unknown'],
    ['click_failure', marker], ['matching_dialog_count', -1], ['matching_dialog_count', 5], ['matching_dialog_count', 1.5],
  ]) {
    const value = leadSample(); value[key] = invalid; assert.equal(parseLead(value), undefined);
  }
  for (const key of ['has_more', 'self_assignable', 'fixture_owner_in_page',
    'expected_display_name_match', 'current_user_id_match', 'owner_option_visible', 'owner_option_enabled',
    'center_hit_self_or_descendant', 'center_hit_another_option', 'center_inside_dialog', 'center_inside_viewport']) {
    for (const invalid of [0, 1, 'true', [], {}]) {
      const value = leadSample(); value[key] = invalid; assert.equal(parseLead(value), undefined);
    }
  }
  for (const change of [value => { value.request_state = 'unobserved'; },
    value => { value.status = null; }, value => { value.status = 500; },
    value => { value.q_scalar_length = null; }, value => { value.returned_count = null; },
    value => { value.option_observation = 'unobserved'; }, value => { value.owner_option_match_count = null; },
    value => { value.you_marker_option_count = null; }, value => { value.owner_option_match_count = 0; }]) {
    const value = leadSample(); change(value); assert.equal(parseLead(value), undefined);
  }
});

test('accepts only fixed click categories and bounded exact geometry objects', () => {
  for (const category of ['pointer_intercepted', 'outside_viewport', 'not_stable', 'detached',
    'not_visible', 'not_enabled', 'strict_match', 'unknown']) {
    const value = leadSample(); value.click_failure = category;
    assert.deepEqual(parseLead(value), value);
  }
  for (const field of ['option_rect', 'popover_rect', 'wrapper_rect', 'trigger_rect', 'dialog_rect',
    'owner_option_content_rect', 'hit_option_rect', 'hit_option_content_rect']) {
    const value = leadSample(); value[field] = { x: -16384, y: 16384, width: 0, height: 16384 };
    value.center_hit_self_or_descendant = false; value.center_inside_dialog = true;
    value.center_inside_viewport = true; value.matching_dialog_count = 1;
    assert.deepEqual(parseLead(value), value);
    for (const bad of [marker, [], {}, { x: 0, y: 0, width: 1, height: 1, id: marker },
      { x: '0', y: 0, width: 1, height: 1 }, { x: -16385, y: 0, width: 1, height: 1 },
      { x: 0, y: 16385, width: 1, height: 1 }, { x: 0, y: 0, width: -1, height: 1 },
      { x: 0, y: 0, width: 1, height: 16385 }, { x: 0, y: 0, width: 0.5, height: 1 },
      { x: 0, y: 0, width: null, height: 1 }]) {
      value[field] = bad; assert.equal(parseLead(value), undefined);
    }
  }
});

test('prints one validated project lead observation at test end before any final receipt', () => {
  const output = [];
  const log = console.log;
  try {
    console.log = value => output.push(String(value));
    const reporter = new ReceiptReporter();
    const testCase = { title: 'Synthetic project lead diagnostic',
      parent: { project: () => ({ name: 'chromium' }) }, location: { file: 'projects.browser.ts', line: 23 } };
    const good = { type: 'project-lead-readiness', description: JSON.stringify(leadSample()) };
    for (const [status, annotations] of [['failed', [good]], ['passed', [good]],
      ['failed', [good, good]], ['failed', [{ ...good, description: JSON.stringify({ ...leadSample(), body: marker }) }]],
      ['failed', [{ type: 'other', description: marker }]], ['failed', [{ ...good, description: marker }]]]) {
      reporter.onTestEnd(testCase, { status, annotations, duration: 10,
        error: { message: marker, stack: `${marker}\n at projects.browser.ts:45:1` },
        stdout: [marker], stderr: [marker], headers: marker, body: marker });
    }
    const diagnostics = output.filter(line => line.startsWith('PROJECT_LEAD_READINESS '));
    assert.equal(diagnostics.length, 2);
    assert.deepEqual(JSON.parse(diagnostics[0].slice(diagnostics[0].indexOf('{'))), leadSample());
    assert.ok(!output.join('\n').includes(marker));
  } finally { console.log = log; }
});

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
