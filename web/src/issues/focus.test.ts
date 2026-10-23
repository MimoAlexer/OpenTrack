// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { restoreIssueRowFocus } from './focus';

let frames: FrameRequestCallback[];
beforeEach(() => {
  frames = [];
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => { frames.push(callback); return frames.length; });
  document.body.innerHTML = '<input aria-label="Search"><a id="row-issue" href="#issue">Issue</a><button id="close">Close issue</button>';
});
afterEach(() => { document.body.innerHTML = ''; vi.unstubAllGlobals(); });
const flush = () => frames.splice(0).forEach(callback => callback(0));
describe('closed issue focus recovery', () => {
  it('returns to the row after the focused detail control unmounts', () => {
    const close = document.getElementById('close')!; close.focus(); close.remove();
    restoreIssueRowFocus('issue'); flush();
    expect(document.activeElement).toBe(document.getElementById('row-issue'));
  });
  it('keeps search focus when a debounced query closes the detail', () => {
    const input = document.querySelector('input')!; input.focus(); input.value = 'release';
    restoreIssueRowFocus('issue'); flush();
    expect(document.activeElement).toBe(input);
    expect(input.value).toBe('release');
  });
  it('does not override focus acquired before the next animation frame', () => {
    restoreIssueRowFocus('issue'); const input = document.querySelector('input')!; input.focus(); flush();
    expect(document.activeElement).toBe(input);
  });
});

it.each(['next', 'previous', 'heading'] as const)('uses the nearest surviving %s after the originating row disappears', destination => {
  document.body.innerHTML = '<h1 tabindex="-1">Issues</h1><div class="issue-groups"><a id="row-before" class="issue-row" href="#before">Before</a><a id="row-issue" class="issue-row" href="#issue">Issue</a><a id="row-after" class="issue-row" href="#after">After</a></div><button id="close">Close issue</button>';
  const heading = document.querySelector('h1')!;
  const close = document.getElementById('close')!; close.focus(); close.remove();
  restoreIssueRowFocus('issue', ['row-before', 'row-issue', 'row-after'], heading);
  document.getElementById('row-issue')!.remove();
  if (destination !== 'next') document.getElementById('row-after')!.remove();
  if (destination === 'heading') document.getElementById('row-before')!.remove();
  flush();
  expect(document.activeElement).toBe(destination === 'heading' ? heading : document.getElementById(destination === 'next' ? 'row-after' : 'row-before'));
});

it('chooses the nearest original neighbor when several earlier rows have disappeared', () => {
  document.body.innerHTML = '<div class="issue-groups"><a id="row-before" class="issue-row" href="#before">Before</a><a id="row-far" class="issue-row" href="#far">Far after</a></div>';
  restoreIssueRowFocus('issue', ['row-gone', 'row-before', 'row-issue', 'row-after-gone', 'row-far']); flush();
  expect(document.activeElement).toBe(document.getElementById('row-before'));
});
