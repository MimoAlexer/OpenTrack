import type { Locator } from '@playwright/test';
import { test, expect, savedBody } from './fixtures';
import type { RichBody } from '../src/documents/model';

// Synthetic clipboard events qualify the browser parser; physical clipboard and IME remain separate.
async function paste(editor: Locator, html: string) {
  await expect(editor).toHaveAttribute('contenteditable', 'true');
  await editor.focus();
  await editor.press('Control+a');
  const handled = await editor.evaluate((element, markup) => {
    const clipboardData = new DataTransfer();
    clipboardData.setData('text/html', markup);
    clipboardData.setData('text/plain', 'Plain-text fallback must not replace the HTML control');
    const event = new ClipboardEvent('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(event, 'clipboardData', { value: clipboardData });
    if (event.clipboardData !== clipboardData || event.clipboardData.getData('text/html') !== markup) throw new Error('The browser did not retain the explicit synthetic HTML clipboard data');
    element.dispatchEvent(event);
    return { prevented: event.defaultPrevented, trusted: event.isTrusted };
  }, html);
  expect(handled).toEqual({ prevented: true, trusted: false });
}

async function expectSafeDOM(editor: Locator, safeHref: string) {
  const unsafe = await editor.evaluate((root, allowedHref) => {
    const elements = Array.from(root.querySelectorAll('*'));
    return {
      active: root.querySelectorAll('script, style, link, meta, base, img, picture, svg, math, iframe, frame, frameset, object, embed, video, audio, source, form, input, button, textarea').length,
      attributes: elements.flatMap(element => Array.from(element.attributes)
        .filter(attribute => /^on/i.test(attribute.name) || ['src', 'srcset', 'srcdoc', 'action', 'formaction', 'data', 'poster', 'style', 'xlink:href'].includes(attribute.name.toLowerCase()))
        .map(attribute => attribute.name)),
      links: elements.filter(element => element.hasAttribute('href'))
        .map(element => element.getAttribute('href')).filter(href => href !== allowedHref),
    };
  }, safeHref);
  expect(unsafe).toEqual({ active: 0, attributes: [], links: [] });
}

test('HTML clipboard parsing preserves formatting while removing active HTML and unsafe URLs before save, reload and read-only display', async ({ page, context, saved }) => {
  const dialogs: string[] = [];
  const foreignRequests: string[] = [];
  const origin = new URL(saved.origin).origin;
  let popups = 0;
  page.on('dialog', dialog => { dialogs.push(dialog.type()); void dialog.dismiss(); });
  page.on('popup', popup => { popups++; void popup.close(); });
  context.on('request', request => {
    if (new URL(request.url()).origin !== origin) foreignRequests.push(request.resourceType());
  });
  await page.addInitScript(() => { Reflect.set(window, '__documentPasteExecuted', false); });
  const issue = await saved.create('HTML clipboard parser qualification');
  await saved.open(page, issue);
  const description = page.getByRole('region', { name: 'Description', exact: true });
  const editor = description.getByRole('textbox', { name: 'Description', exact: true });
  const safeHref = `${origin}/paste-positive-control`;
  const control: RichBody = { type: 'doc', content: [{ type: 'paragraph', content: [
    { type: 'text', text: 'Clipboard ' },
    { type: 'text', text: 'formatting', marks: [{ type: 'bold' }] },
    { type: 'text', text: ' and ' },
    { type: 'text', text: 'safe link', marks: [{ type: 'link', attrs: { href: safeHref } }] },
  ] }] };
  const execute = 'window.__documentPasteExecuted=true;alert("paste-executed")';
  const foreign = 'https://paste-probe.invalid/blocked';
  const cases: { name: string; html: string; expected: RichBody; bold: string; link: boolean }[] = [
    { name: 'benign HTML formatting control',
      html: `<p>Clipboard <strong>formatting</strong> and <a href="${safeHref}">safe link</a></p>`,
      expected: control, bold: 'formatting', link: true },
    { name: 'active elements and event attributes',
      html: `<p style="background-image:url(${foreign})" onmouseover='${execute}'>Active HTML <strong onclick='${execute}'>survives</strong></p>`
        + `<script>${execute}</script><img src="${foreign}" onerror='${execute}'>`
        + `<svg onload='${execute}'><use href="${foreign}"></use></svg>`
        + `<iframe src="${foreign}" srcdoc="<script>parent.__documentPasteExecuted=true</script>"></iframe>`
        + `<object data="${foreign}"></object><embed src="${foreign}">`
        + `<style>@import url(${foreign});</style><link rel="stylesheet" href="${foreign}">`
        + `<form action="${foreign}"><input autofocus onfocus='${execute}'></form>`,
      expected: { type: 'doc', content: [{ type: 'paragraph', content: [
        { type: 'text', text: 'Active HTML ' }, { type: 'text', text: 'survives', marks: [{ type: 'bold' }] },
      ] }] }, bold: 'survives', link: false },
    { name: 'script and other unsupported URL schemes',
      html: `<p>Unsafe links: <a href='javascript:${execute}'>script</a> `
        + `<a href='jav&#x61;script:${execute}'>entity</a> `
        + `<a href='java&#x09;script:${execute}'>control</a> `
        + `<a href="data:text/html,%3Cscript%3Eparent.__documentPasteExecuted=true%3C/script%3E">data</a> `
        + `<a href="vbscript:msgbox(1)">legacy</a> <a href="file:///qualification-only">local</a></p>`,
      expected: { type: 'doc', content: [{ type: 'paragraph', content: [
        { type: 'text', text: 'Unsafe links: script entity control data legacy local' },
      ] }] }, bold: '', link: false },
  ];
  async function inert() {
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    expect(await page.evaluate(() => Reflect.get(window, '__documentPasteExecuted'))).toBe(false);
    expect(dialogs).toEqual([]);
    expect(foreignRequests).toEqual([]);
    expect(popups).toBe(0);
  }
  for (const entry of cases) {
    await test.step(entry.name, async () => {
      await paste(editor, entry.html);
      const expectedText = entry.expected.content[0].content!.map(node => node.text).join('');
      await expect(editor).toHaveText(expectedText);
      await expect(editor.locator('strong')).toHaveText(entry.bold ? [entry.bold] : []);
      await expect(editor.locator('a')).toHaveCount(entry.link ? 1 : 0);
      await expectSafeDOM(editor, safeHref);
      await inert();
      await savedBody(saved, issue.id, entry.expected);
      expect((await saved.document(issue.id)).body).toEqual(entry.expected);
      await inert();
      await page.reload();
      await expect(editor).toBeVisible();
      await expect(editor).toHaveText(expectedText);
      await expect(editor.locator('strong')).toHaveText(entry.bold ? [entry.bold] : []);
      await expect(editor.locator('a')).toHaveCount(entry.link ? 1 : 0);
      await expectSafeDOM(editor, safeHref);
      expect((await saved.document(issue.id)).body).toEqual(entry.expected);
      await inert();
      await page.getByRole('button', { name: 'Archive', exact: true }).click();
      await expect.poll(async () => (await saved.issue(issue.id)).archived_at).not.toBeNull();
      await expect(editor).toHaveCount(0);
      const view = description.locator('.document-prose');
      await expect(view).toBeVisible();
      await expect(view).not.toHaveAttribute('contenteditable', 'true');
      await expect(view).toHaveText(expectedText);
      await expect(view.locator('strong')).toHaveText(entry.bold ? [entry.bold] : []);
      await expect(view.locator('a')).toHaveCount(entry.link ? 1 : 0);
      if (entry.link) await expect(view.getByRole('link', { name: 'safe link', exact: true })).toHaveAttribute('href', safeHref);
      await expectSafeDOM(view, safeHref);
      expect((await saved.document(issue.id)).body).toEqual(entry.expected);
      await inert();
      await page.getByRole('button', { name: 'Restore', exact: true }).click();
      const restore = page.getByRole('dialog', { name: `Restore ${issue.key}`, exact: true });
      await expect(restore).toBeVisible();
      await restore.getByRole('button', { name: 'Restore issue', exact: true }).click();
      await expect(restore).toHaveCount(0);
      await expect.poll(async () => (await saved.issue(issue.id)).archived_at).toBeNull();
      await expect(editor).toBeVisible();
      await expect(editor).toHaveText(expectedText);
      await expect(editor.locator('strong')).toHaveText(entry.bold ? [entry.bold] : []);
      await expect(editor.locator('a')).toHaveCount(entry.link ? 1 : 0);
      await expectSafeDOM(editor, safeHref);
      expect((await saved.document(issue.id)).body).toEqual(entry.expected);
      await inert();
    });
  }
});
