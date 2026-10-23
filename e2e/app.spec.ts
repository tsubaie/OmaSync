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

test('a sync shows its steps once, then clears them', async ({ page }) => {
  await page.goto('/?scenario=syncing');
  await page.getByRole('button', { name: 'Sync Now' }).click();
  // While it runs: the steps so far, the upload's percentage, and no second sync.
  await expect(page.getByText('Sharing with your other computers 62%')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Syncing…' })).toBeDisabled();

  // A finished round (sync, then the look afterwards) leaves nothing behind.
  await page.goto('/?scenario=synced');
  await page.getByRole('button', { name: 'Sync Now' }).click();
  await expect(page.locator('.toast').getByText('Synced')).toBeVisible();
  await expect(page.locator('.progress')).toBeHidden();
  await expect(page.getByRole('button', { name: 'Sync Now' })).toBeEnabled();
});

test('every pane is reachable from the keyboard and passes axe', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await expect(page.getByRole('tab', { name: 'Sync', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  );
  for (const [key, name] of [
    ['2', 'History'],
    ['3', 'Configurations'],
    ['4', 'Plugins'],
    ['5', 'Installed Apps'],
    ['6', 'App Data'],
    ['7', 'Settings'],
  ]) {
    await page.keyboard.press(`Control+${key}`);
    await expect(page.getByRole('heading', { level: 1, name })).toBeVisible();
    expect(await a11y(page), name).toEqual([]);
  }
  // Arrow keys move through the sidebar like a macOS source list.
  await page.getByRole('tab', { name: 'App Data' }).focus();
  await page.keyboard.press('ArrowUp');
  await expect(page.getByRole('tab', { name: 'Installed Apps' })).toBeFocused();
});

test('switching an app off for a computer saves the choice', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'Installed Apps' }).click();
  const yazi = page.getByRole('switch', { name: 'yazi synced on This Computer' });
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
  // Each sync lists the files it changed, and Undo shows them before going ahead.
  await page.getByRole('button', { name: 'Show 2 Files' }).click();
  await expect(page.getByText('Changed bindings.lua')).toBeVisible();
  await expect(page.getByText('Added input.lua')).toBeVisible();
  await page.getByRole('button', { name: 'Undo', exact: true }).click();
  await expect(page.getByText('Undo puts these back as they were before this sync')).toBeVisible();
  await expect(page.getByText('Changed bindings.lua')).toBeVisible();
  await expect(page.getByText('Added input.lua')).toBeVisible();
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

test("another computer's plugin waits for a yes before it's added here", async ({ page }) => {
  await page.goto('/?scenario=synced');
  const waiting = page.getByRole('region', { name: 'Plugins Waiting to Add' });
  await expect(waiting.getByText('Webcam Controls')).toBeVisible();
  await expect(waiting.getByText('On Desktop')).toBeVisible();
  await waiting.getByRole('button', { name: 'Add' }).click();
  expect(await calls(page, 'install_plugins')).toEqual([{ cmd: 'install_plugins', args: {} }]);
  await expect(waiting).toBeHidden();
});

test('an app is switched on per computer, in place', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'Installed Apps' }).click();
  const rows = page.locator('.matrix:not(.head) .text > div:first-child');
  await expect(rows).toHaveText(['brave-bin', 'spotify', 'tapecalc', 'yazi']);
  await expect(page.getByText('Only on this computer').first()).toBeVisible();
  const desk = page.getByRole('switch', { name: 'spotify synced on Desktop' });
  await expect(desk).not.toBeChecked();
  // Switching Desktop on shares it, and this computer (which has it) comes along.
  await desk.focus();
  await page.keyboard.press('Space');
  await expect(desk).toBeChecked();
  await expect(page.getByRole('switch', { name: 'spotify synced on This Computer' })).toBeChecked();
  await expect(rows).toHaveText(['brave-bin', 'spotify', 'tapecalc', 'yazi']);
  const banner = page.getByText('Your other computers get this change the next time you sync.');
  await expect(banner).toBeVisible();
  // Every computer switched off again: nothing is left to share, so the banner goes away.
  const here = page.getByRole('switch', { name: 'spotify synced on This Computer' });
  await page.keyboard.press('Space');
  await here.focus();
  await page.keyboard.press('Space');
  await expect(desk).not.toBeChecked();
  await expect(here).not.toBeChecked();
  await expect(banner).toBeHidden();
  // Switched on for this computer alone, it stays on: computers added later get it.
  await page.keyboard.press('Space');
  await expect(here).toBeChecked();
});

test('only Sync opens until the first check finishes', async ({ page }) => {
  await page.goto('/?scenario=checking');
  await expect(page.getByRole('tab', { name: 'History' })).toHaveAttribute('aria-disabled', 'true');
  await expect(page.getByRole('tab', { name: 'Sync', exact: true })).not.toHaveAttribute(
    'aria-disabled',
    'true',
  );
  await page.keyboard.press('Control+2');
  await expect(page.getByRole('heading', { level: 1, name: 'Sync' })).toBeVisible();
});

test('each plugin is switched on per computer, like apps', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'Plugins' }).click();
  const desk = page.getByRole('switch', { name: 'Webcam Controls synced on Desktop' });
  await expect(desk).toBeChecked();
  await desk.focus();
  await page.keyboard.press('Space');
  await expect(desk).not.toBeChecked();
  expect(await calls(page, 'set_plugin')).toEqual([
    { cmd: 'set_plugin', args: { computer: 'bbbb33334444', plugin: 'webcam', on: false } },
  ]);
});

test('an app from outside the repos syncs only once it has an install script', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'Installed Apps' }).click();
  await expect(
    page.getByText("Not in Arch's repos or the AUR, so other computers need"),
  ).toBeVisible();
  const desk = page.getByRole('switch', { name: 'tapecalc synced on Desktop' });
  await expect(desk).toBeDisabled();

  // The form starts empty; the person types where it comes from and what to run.
  await page.getByRole('button', { name: 'Add an Install Script…' }).click();
  const repo = page.getByRole('textbox', { name: 'Git repository' });
  const command = page.getByRole('textbox', { name: 'Command to run in it' });
  await expect(repo).toHaveValue('');
  await expect(command).toHaveValue('');
  await repo.fill('https://example.com/tapecalc');
  await command.fill('./install.sh');
  expect(await a11y(page)).toEqual([]);
  await page.getByRole('button', { name: 'Save Install Script' }).click();
  expect(await calls(page, 'set_app_script')).toEqual([
    {
      cmd: 'set_app_script',
      args: { app: 'tapecalc', repo: 'https://example.com/tapecalc', command: './install.sh' },
    },
  ]);
  await expect(
    page.getByText('Installs with “./install.sh” from https://example.com/tapecalc'),
  ).toBeVisible();
  await expect(desk).toBeEnabled();
});
