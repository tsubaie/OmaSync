import { AxeBuilder } from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';

/** WCAG A and AA problems on the current screen, as readable lines. */
async function a11y(page: Page) {
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'])
    .analyze();
  return result.violations.map((v) => `${v.id}: ${v.help} (${v.nodes.length})`);
}

const calls = (page: Page, cmd: string) =>
  page.evaluate((c) => window.e2eCalls.filter((x) => x.cmd === c), cmd);

test('first run names exactly what it installs', async ({ page }) => {
  await page.goto('/?scenario=setup');
  await expect(page.getByText('Step 1 of 3')).toBeVisible();
  await expect(
    page.getByText('Copies your settings files to and from your private sync space.', {
      exact: false,
    }),
  ).toBeVisible();
  await expect(page.getByRole('button', { name: 'Install chezmoi' })).toBeVisible();
  expect(await a11y(page)).toEqual([]);
});

test('sync pane shows the state and syncs on request', async ({ page }) => {
  await page.goto('/?scenario=synced');
  const status = page
    .getByRole('status')
    .filter({ hasText: 'Everything matches your other computers.' });
  await expect(status).toBeVisible();
  expect(await a11y(page)).toEqual([]);

  await page.getByRole('button', { name: 'Sync Now' }).click();
  // Success is a toast that slides away.
  await expect(page.locator('.toast').getByText('Synced')).toBeVisible();
  expect(await calls(page, 'sync_now')).toEqual([
    expect.objectContaining({ args: expect.objectContaining({ prefer: 'Ask' }) }),
  ]);
});

test('every pane is reachable from the keyboard and passes axe', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await expect(page.getByRole('tab', { name: 'Sync', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  );
  for (const [key, name] of [
    ['2', 'Omarchy'],
    ['3', 'Plugins'],
    ['4', 'Apps'],
    ['5', 'App Data'],
    ['6', 'History'],
    ['7', 'Settings'],
  ]) {
    await page.keyboard.press(`Control+${key}`);
    await expect(page.getByRole('heading', { level: 1, name })).toBeVisible();
    expect(await a11y(page), name).toEqual([]);
  }
  // Arrow keys move through the sidebar like a macOS source list.
  await page.getByRole('tab', { name: 'History' }).focus();
  await page.keyboard.press('ArrowUp');
  await expect(page.getByRole('tab', { name: 'App Data' })).toBeFocused();
});

test('switching an app off for a computer saves the choice', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'Apps' }).click();
  const yazi = page.getByRole('switch', { name: 'yazi on This Computer' });
  await expect(yazi).toBeChecked();
  // Toggle it the way a keyboard user does; it works the same in WebKit and Chromium.
  await yazi.focus();
  await page.keyboard.press('Space');
  await expect(yazi).not.toBeChecked();
  expect(await calls(page, 'set_app')).toEqual([
    { cmd: 'set_app', args: { computer: 'aaaa11112222', app: 'yazi', on: false } },
  ]);
  await expect(
    page.getByText('Your other computers get this change the next time you sync.'),
  ).toBeVisible();
});

test('a conflict offers two equal choices naming the other computer', async ({ page }) => {
  await page.goto('/?scenario=conflict');
  await page.getByRole('button', { name: 'Sync Now' }).click();
  await expect(page.getByText('The same setting was changed on two computers.')).toBeVisible();
  await expect(page.getByRole('button', { name: "Keep This Computer's Version" })).toBeVisible();
  await page.getByRole('button', { name: "Keep Desktop's Version" }).click();
  expect((await calls(page, 'sync_now')).map((c) => c.args.prefer)).toEqual([
    'Ask',
    'OtherComputer',
  ]);
  expect(await a11y(page)).toEqual([]);
});

test('undo asks before changing every computer', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'History' }).click();
  await page.getByRole('button', { name: 'Undo', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Undo on All Computers' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: 'Undo on All Computers' })).toBeHidden();
  expect(await calls(page, 'undo')).toEqual([]);
});

test('settings names this computer and shows the sync space', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await expect(page.getByText('Sign in to the same GitHub account')).toBeVisible();
  await page.keyboard.press('Control+7');
  await expect(page.getByRole('heading', { level: 1, name: 'Settings' })).toBeVisible();
  await expect(page.getByText('github.com/someone/omarchy-sync')).toBeVisible();
  expect(await a11y(page)).toEqual([]);

  const name = page.getByRole('textbox', { name: "This computer's name" });
  await name.fill('ThinkPad');
  await name.press('Enter');
  expect(await calls(page, 'rename_computer')).toEqual([
    { cmd: 'rename_computer', args: { name: 'ThinkPad' } },
  ]);

  // Removing another computer asks first.
  await page.getByRole('button', { name: 'Remove…' }).click();
  await expect(page.getByRole('button', { name: 'Remove', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Cancel' }).click();
  expect(await calls(page, 'forget_computer')).toEqual([]);
});
