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
