import { randomUUID } from 'node:crypto';
import type { Locator, Page } from '@playwright/test';
import { test, expect } from './fixtures';

async function insideViewport(target: Locator, page: Page) {
  const box = await target.boundingBox();
  expect(box).not.toBeNull();
  const viewport = page.viewportSize()!;
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(viewport.width + 1);
  expect(box!.y + box!.height).toBeLessThanOrEqual(viewport.height + 1);
}

async function tokens(page: Page) {
  return page.evaluate(() => {
    const probe = document.createElement('span'); document.body.append(probe);
    const color = (token: string) => {
      probe.style.color = `var(--${token})`;
      return getComputedStyle(probe).color.match(/[\d.]+/g)!.slice(0, 3).map(Number);
    };
    const luminance = (rgb: number[]) => rgb.map(value => value / 255)
      .map(value => value <= .04045 ? value / 12.92 : ((value + .055) / 1.055) ** 2.4)
      .reduce((sum, value, index) => sum + value * [.2126, .7152, .0722][index], 0);
    const pairs: [string, string, number][] = [];
    for (const text of ['text', 'secondary', 'muted']) for (const surface of ['bg', 'side', 'panel', 'hover', 'selected']) pairs.push([text, surface, 4.5]);
    for (const text of ['green', 'amber', 'red', 'accent']) pairs.push([text, `${text}-bg`, 4.5]);
    pairs.push(['solid-text', 'solid', 4.5]);
    for (const foreground of ['accent', 'control']) for (const surface of ['bg', 'side', 'panel', 'hover', 'selected']) pairs.push([foreground, surface, 3]);
    const result = pairs.map(([foreground, background, minimum]) => {
      const a = luminance(color(foreground)); const b = luminance(color(background));
      return { foreground, background, minimum, ratio: (Math.max(a, b) + .05) / (Math.min(a, b) + .05) };
    });
    probe.remove(); return result;
  });
}

for (const theme of ['light', 'dark']) {
  test(`${theme} shared primitives meet contrast, modal focus and saved combobox selection`, async ({ page, saved }) => {
    await page.addInitScript(value => localStorage.setItem('opentrack:theme', value), theme);
    await saved.open(page);
    for (const pair of await tokens(page)) expect(pair.ratio, `${pair.foreground} on ${pair.background}`).toBeGreaterThanOrEqual(pair.minimum);
    const trigger = page.getByRole('button', { name: /^New issue/ });
    await trigger.click();
    const dialog = page.getByRole('dialog', { name: 'New issue', exact: true });
    const title = dialog.getByRole('textbox', { name: 'Title', exact: true });
    await expect(title).toBeFocused();
    await expect(dialog.getByRole('textbox', { name: 'Description', exact: true })).toBeVisible();
    const close = dialog.getByRole('button', { name: 'Close dialog', exact: true });
    await close.focus();
    await close.press('Shift+Tab');
    await expect(dialog.getByRole('button', { name: 'Cancel', exact: true })).toBeFocused();
    await page.keyboard.press('Tab');
    await expect(close).toBeFocused();
    const focused = dialog.locator(':focus-visible');
    await expect(focused).toHaveCount(1);
    expect(await focused.evaluate(element => getComputedStyle(element).outlineStyle)).toBe('solid');
    const assignee = dialog.getByRole('button', { name: /^Assignee/ });
    await assignee.press('Enter');
    const search = dialog.getByRole('combobox', { name: 'Search assignee', exact: true });
    await expect(search).toBeFocused();
    await expect(dialog.getByRole('option', { name: /Assign to me/ })).toBeVisible();
    await search.press('End');
    await expect(search).toHaveAttribute('aria-activedescendant', /.+/);
    const last = await search.getAttribute('aria-activedescendant');
    expect(last).toBeTruthy();
    await expect(dialog.locator(`[id="${last}"]`)).not.toHaveAttribute('aria-disabled', 'true');
    await search.press('Home');
    await expect(search).toHaveAttribute('aria-activedescendant', (await dialog.locator('[role=option]').first().getAttribute('id'))!);
    await search.press('ArrowDown');
    await search.press('Enter');
    await expect(search).toHaveCount(0);
    await expect(assignee).toBeFocused();
    await expect(assignee).toContainText('Example Owner');
    await assignee.press('Enter');
    await expect(search).toBeFocused();
    await search.press('Escape');
    await expect(dialog).toBeVisible();
    await expect(assignee).toBeFocused();
    const name = `Primitive acceptance ${randomUUID()}`;
    await title.fill(name);
    let release!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; });
    await page.route(`**${saved.path}/issues`, async route => {
      if (route.request().method() === 'POST') await gate;
      await route.continue();
    });
    const created = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === `${saved.origin}${saved.path}/issues`);
    try {
      await dialog.getByRole('button', { name: 'Create issue', exact: true }).click();
      await expect(dialog.getByRole('button', { name: /^Creating/ })).toBeDisabled();
      await expect(title).toBeDisabled();
      await expect(assignee).toBeDisabled();
    } finally { release(); }
    const result = await created;
    expect(result.status()).toBe(201);
    const issue = await result.json() as { id: string };
    expect(await saved.issue(issue.id)).toMatchObject({ title: name, assignee_id: saved.session.user.id });
    await expect(dialog).toHaveCount(0);
  });
}

for (const width of [320, 640]) {
  test(`shared issue form stays operable at ${width}px reflow with enlarged text`, async ({ page, saved }) => {
    await page.setViewportSize({ width, height: 740 });
    await saved.open(page);
    const trigger = page.getByRole('button', { name: /^New issue/ });
    await trigger.click();
    const dialog = page.getByRole('dialog', { name: 'New issue', exact: true });
    if (width === 640) {
      // Explicit text-only stress check; browser page zoom is separately represented by the 320px viewport.
      await dialog.evaluate(element => {
        const targets = [...element.querySelectorAll<HTMLElement>('*')].filter(node =>
          node.matches('input, select, textarea') || [...node.childNodes].some(child => child.nodeType === Node.TEXT_NODE && !!child.textContent?.trim()));
        const sizes = targets.map(node => ({ node, size: Number.parseFloat(getComputedStyle(node).fontSize), line: Number.parseFloat(getComputedStyle(node).lineHeight) }));
        for (const { node, size, line } of sizes) { node.style.fontSize = `${size * 2}px`; if (Number.isFinite(line)) node.style.lineHeight = `${line * 2}px`; }
      });
    }
    await insideViewport(dialog, page);
    expect(await dialog.evaluate(element => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
    const title = dialog.getByRole('textbox', { name: 'Title', exact: true });
    await title.fill('A long reviewed title that must retain the complete editable value at narrow widths');
    const assignee = dialog.getByRole('button', { name: /^Assignee/ });
    await assignee.click();
    const search = dialog.getByRole('combobox', { name: 'Search assignee', exact: true });
    await expect(search).toBeFocused();
    await expect(dialog.getByRole('option', { name: /Assign to me/ })).toBeVisible();
    await insideViewport(dialog.locator('.reference-popover'), page);
    await search.fill('no-match-synthetic');
    await expect(search).toHaveValue('no-match-synthetic');
    await search.press('Escape');
    await expect(assignee).toBeFocused();
    await dialog.getByRole('button', { name: /^Labels/ }).click();
    const labelSearch = dialog.getByRole('combobox', { name: 'Search labels', exact: true });
    await labelSearch.fill('no-match-synthetic');
    await expect(dialog.getByText('No matches.', { exact: true })).toBeVisible();
    await labelSearch.press('Escape');
    const cancel = dialog.getByRole('button', { name: 'Cancel', exact: true });
    await cancel.scrollIntoViewIfNeeded();
    await insideViewport(cancel, page);
    await cancel.click();
    const discard = page.getByRole('dialog', { name: 'Discard this issue draft?', exact: true });
    await expect(discard).toBeVisible();
    await discard.getByRole('button', { name: 'Keep editing', exact: true }).click();
    await expect(title).toHaveValue('A long reviewed title that must retain the complete editable value at narrow widths');
    await cancel.click();
    await discard.getByRole('button', { name: 'Discard draft', exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await expect(trigger).toBeFocused();
  });
}

test('forced colors and reduced motion retain visible native focus', async ({ page, saved }) => {
  await page.emulateMedia({ forcedColors: 'active', reducedMotion: 'reduce' });
  await saved.open(page);
  await page.getByRole('button', { name: /^New issue/ }).click();
  const dialog = page.getByRole('dialog', { name: 'New issue', exact: true });
  const title = dialog.getByRole('textbox', { name: 'Title', exact: true });
  await expect(title).toBeFocused();
  await title.press('Shift+Tab');
  const focus = dialog.locator(':focus-visible');
  expect(await focus.evaluate(element => getComputedStyle(element).outlineWidth)).toBe('2px');
  expect(await dialog.evaluate(element => [...element.querySelectorAll('*')].every(node => getComputedStyle(node).animationName === 'none'))).toBe(true);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
});
