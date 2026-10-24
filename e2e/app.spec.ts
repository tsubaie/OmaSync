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
  expect(await calls(page, 'sync_now')).toHaveLength(1);
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
    ['2', 'Configurations'],
    ['3', 'Plugins'],
    ['4', 'Repositories'],
    ['5', 'Installed Apps'],
    ['6', 'App Data'],
    ['7', 'History'],
    ['8', 'Settings'],
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
  // An Arch or AUR app links to its package page; one from Omarchy's repository (no pages)
  // to its own website; one with install steps has neither.
  await page.getByRole('button', { name: 'Open archlinux.org (yazi)' }).click();
  await page.getByRole('button', { name: 'Open the AUR (spotify)' }).click();
  await page.getByRole('button', { name: 'Open brave.com (brave-bin)' }).click();
  expect(await calls(page, 'open_page')).toEqual([
    { cmd: 'open_page', args: { kind: 'arch', name: 'yazi' } },
    { cmd: 'open_page', args: { kind: 'aur', name: 'spotify' } },
    { cmd: 'open_page', args: { kind: 'website', name: 'brave-bin' } },
  ]);
  await expect(page.getByRole('button', { name: /\(tapecalc\)$/ })).toHaveCount(0);
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

test('a conflict shows both versions and keeps the one chosen for each setting', async ({
  page,
}) => {
  await page.goto('/?scenario=conflict');
  await page.getByRole('button', { name: 'Sync Now' }).click();
  await expect(page.getByText('The same setting was changed on two computers.')).toBeVisible();
  // Both versions side by side, with the differing lines marked.
  const keys = page.getByRole('table', { name: 'Both versions of .config/hypr/bindings.lua' });
  await expect(keys.getByRole('columnheader', { name: 'This Computer' })).toBeVisible();
  await expect(keys.getByRole('columnheader', { name: 'Desktop' })).toBeVisible();
  await expect(keys.locator('tr.changed')).toHaveCount(2);
  expect(await a11y(page)).toEqual([]);

  // One choice per setting; keeping waits until every setting has one.
  // The button says how many settings still need a choice.
  await expect(page.getByRole('button', { name: 'Choose 2 More' })).toBeDisabled();
  await page
    .getByRole('radiogroup', { name: 'Which version of .config/hypr/bindings.lua to keep' })
    .getByText("Keep Desktop's Version")
    .click();
  await expect(page.getByRole('button', { name: 'Choose 1 More' })).toBeDisabled();
  await page
    .getByRole('radiogroup', { name: 'Which version of .config/ghostty/config to keep' })
    .getByText("Keep This Computer's Version")
    .click();
  await page.getByRole('button', { name: 'Keep Chosen Versions' }).click();
  expect(await calls(page, 'resolve_conflict')).toEqual([
    expect.objectContaining({
      args: expect.objectContaining({
        choices: { 'dot_config/hypr/bindings.lua': 'Theirs', 'dot_config/ghostty/config': 'Mine' },
      }),
    }),
  ]);
});

test('cancelling a clash changes nothing and leaves it for the next sync', async ({ page }) => {
  await page.goto('/?scenario=conflict');
  await page.getByRole('button', { name: 'Sync Now' }).click();
  await expect(page.getByText('Changed on Both Computers')).toBeVisible();
  await page.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByText('Changed on Both Computers')).toBeHidden();
  expect(await calls(page, 'resolve_conflict')).toEqual([]);
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
  // Undo never deletes a file, so it says so when the sync added one.
  await expect(page.getByText('Files it added stay on each computer')).toBeVisible();
  await expect(page.getByText('Changed bindings.lua')).toBeVisible();
  await expect(page.getByText('Added input.lua')).toBeVisible();
});

test('settings names this computer and shows the sync space', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await expect(page.getByText('Sign in to the same GitHub account')).toBeVisible();
  await page.keyboard.press('Control+8');
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
  await expect(rows).toHaveText([/^brave-bin/, /^quicknote/, /^spotify/, /^tapecalc/, /^yazi/]);
  await expect(page.getByText('Only on this computer').first()).toBeVisible();
  // Each app says where it comes from; the AUR is the last resort.
  await expect(page.locator('.source.aur')).toHaveText('AUR');
  await expect(page.locator('.source.omarchy')).toHaveText('Omarchy');
  const desk = page.getByRole('switch', { name: 'spotify synced on Desktop' });
  await expect(desk).not.toBeChecked();
  // Switching Desktop on shares it, and this computer (which has it) comes along.
  await desk.focus();
  await page.keyboard.press('Space');
  await expect(desk).toBeChecked();
  await expect(page.getByRole('switch', { name: 'spotify synced on This Computer' })).toBeChecked();
  await expect(rows).toHaveText([/^brave-bin/, /^quicknote/, /^spotify/, /^tapecalc/, /^yazi/]);
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

test('plugins are switched on, added from git, updated and removed here', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'Plugins' }).click();
  expect(await a11y(page)).toEqual([]);
  // Omarchy's own plugins only go on and off; a plugin from git can be updated and removed.
  const clock = page.getByRole('switch', { name: 'Clock enabled' });
  await expect(clock).toBeChecked();
  await clock.focus();
  await page.keyboard.press('Space');
  expect(await calls(page, 'set_plugin_enabled')).toEqual([
    { cmd: 'set_plugin_enabled', args: { id: 'omarchy.clock', on: false } },
  ]);
  await expect(page.getByText('From github.com/sspaeti/omarchy-plugin-timezones')).toBeVisible();
  // Its source opens in the browser; Rust turns the git address into the page.
  await page
    .getByRole('button', { name: 'From github.com/sspaeti/omarchy-plugin-timezones' })
    .click();
  expect(await calls(page, 'open_page')).toEqual([
    {
      cmd: 'open_page',
      args: { kind: 'plugin', name: 'https://github.com/sspaeti/omarchy-plugin-timezones.git' },
    },
  ]);
  await page.getByRole('button', { name: 'Update Plugin', exact: true }).click();
  await expect(page.getByText('timezones is up to date.')).toBeVisible();
  expect(await calls(page, 'update_plugins')).toEqual([
    { cmd: 'update_plugins', args: { id: 'timezones' } },
  ]);
  // Adding asks for the address, warns, and only then runs.
  await page.getByRole('button', { name: 'Add Plugin…' }).click();
  await expect(page.getByText('Only add plugins from a source you fully trust.')).toBeVisible();
  const add = page.getByRole('button', { name: 'Add Plugin', exact: true });
  await expect(add).toBeDisabled();
  await page.getByLabel('Git address').fill('https://github.com/acme/omarchy-weather.git');
  await add.click();
  expect(await calls(page, 'add_plugin')).toEqual([
    { cmd: 'add_plugin', args: { url: 'https://github.com/acme/omarchy-weather.git' } },
  ]);
  // Removing asks first and says it's this computer only.
  await page.getByRole('button', { name: 'Remove Plugin…' }).click();
  await expect(page.getByText('Removes Timezones from this computer only.')).toBeVisible();
  await page.getByRole('button', { name: 'Remove from This Computer' }).click();
  expect(await calls(page, 'remove_plugin')).toEqual([
    { cmd: 'remove_plugin', args: { id: 'timezones' } },
  ]);
});

test('an app from outside the repos syncs only once it has an install command', async ({
  page,
}) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'Installed Apps' }).click();
  await expect(
    page.getByText("Not in Arch's repos or the AUR. Add its install steps to sync it."),
  ).toBeVisible();
  // Every switch stays locked until it has one, this computer's too.
  const here = page.getByRole('switch', { name: 'tapecalc synced on This Computer' });
  const desk = page.getByRole('switch', { name: 'tapecalc synced on Desktop' });
  await expect(here).toBeDisabled();
  await expect(desk).toBeDisabled();

  // One field, empty: the person types the command that installs it.
  await page.getByRole('button', { name: 'Add Install Steps…' }).click();
  const command = page.getByRole('textbox', { name: 'Install steps' });
  await expect(command).toHaveValue('');
  await expect(page.getByRole('button', { name: 'Save Install Steps' })).toBeDisabled();
  await command.fill('git clone https://example.com/tapecalc\ncd tapecalc\n./install.sh');
  expect(await a11y(page)).toEqual([]);
  await page.getByRole('button', { name: 'Save Install Steps' }).click();
  expect(await calls(page, 'set_app_script')).toEqual([
    {
      cmd: 'set_app_script',
      args: {
        app: 'tapecalc',
        command: 'git clone https://example.com/tapecalc\ncd tapecalc\n./install.sh',
      },
    },
  ]);
  await expect(
    page.getByText(
      "Not in Arch's repos or the AUR. Installs with 3 steps, starting “git clone https://example.com/tapecalc”.",
    ),
  ).toBeVisible();
  await expect(here).toBeEnabled();
  await expect(desk).toBeEnabled();
});

test('an install command runs only after a warning and a second yes', async ({ page }) => {
  await page.goto('/?scenario=synced');
  const waiting = page.getByRole('region', { name: 'Apps to Install with Their Steps' });
  await expect(waiting.getByText('quicknote', { exact: true })).toBeVisible();
  await waiting.getByRole('button', { name: 'Install…' }).click();
  await expect(waiting.getByText('Run this only if you trust where it came from.')).toBeVisible();
  await expect(waiting.locator('code')).toHaveText(
    'curl -fsSL https://example.com/quicknote.sh | bash',
  );
  expect(await calls(page, 'install_with_script')).toEqual([]);
  expect(await a11y(page)).toEqual([]);
  await waiting.getByRole('button', { name: 'Run It' }).click();
  expect(await calls(page, 'install_with_script')).toEqual([
    expect.objectContaining({ args: expect.objectContaining({ app: 'quicknote' }) }),
  ]);
});

test('the bar indicator is on by default and flips off at once', async ({ page }) => {
  await page.goto('/?scenario=synced');
  // Pane shortcuts only work once the page is ready; the first check must have answered.
  await expect(page.getByText('Sign in to the same GitHub account')).toBeVisible();
  await page.keyboard.press('Control+8');
  const bar = page.getByRole('switch', { name: 'Show in the bar' });
  await expect(bar).toBeChecked();
  await bar.focus();
  await page.keyboard.press('Space');
  await expect(bar).not.toBeChecked();
  expect(await calls(page, 'set_bar_indicator')).toEqual([
    { cmd: 'set_bar_indicator', args: { on: false } },
  ]);
});

test('problems and ideas can be reported from beside Settings', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('button', { name: 'Report a Problem or Suggest Something' }).click();
  expect(await calls(page, 'report')).toEqual([
    { cmd: 'report', args: { message: null, details: null } },
  ]);
});

test("an app from its maker's repository waits until the repository is added", async ({ page }) => {
  await page.goto('/?scenario=repo');
  // The Sync page points to the Repositories section, where they are added.
  await page.getByRole('button', { name: 'Show Repositories' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Repositories' })).toBeVisible();
  const group = page.getByRole('region', { name: 'Repositories to Add' });
  await expect(group.getByText('Needed for chatgpt-bin.', { exact: false })).toBeVisible();
  await group.getByRole('button', { name: 'Add Repository…' }).click();
  // Nothing runs until the person has seen whose repository it is and its key, and agreed.
  await expect(group.getByText('Add it only if you trust Codex Linux Repository.')).toBeVisible();
  await expect(group.locator('code')).toHaveText(
    '3BFA 0E4A E8B8 CC16 A2D9 BA68 4A3B 4A56 6C46 60E4',
  );
  expect(await calls(page, 'add_repository')).toEqual([]);
  expect(await a11y(page)).toEqual([]);
  await group.getByRole('button', { name: 'Add It' }).click();
  expect(await calls(page, 'add_repository')).toEqual([
    { cmd: 'add_repository', args: { name: 'openai-chatgpt' } },
  ]);
  // Or by hand: the same steps as terminal commands.
  await group.getByRole('button', { name: 'Show Commands' }).click();
  await expect(group.locator('.commands code')).toContainText(
    'sudo pacman-key --lsign-key 3BFA0E4AE8B8CC16A2D9BA684A3B4A566C4660E4',
  );
  // Until then the app is off and locked here.
  await page.getByRole('tab', { name: 'Installed Apps' }).click();
  await expect(
    page.getByText('Needs the openai-chatgpt repository on this computer first'),
  ).toBeVisible();
  await expect(
    page.getByRole('switch', { name: 'chatgpt-bin synced on This Computer' }),
  ).toBeDisabled();
  await expect(
    page.getByRole('switch', { name: 'chatgpt-bin synced on This Computer' }),
  ).not.toBeChecked();
});

test('an older Omarchy keeps its own settings, can try the newer ones, and put them back', async ({
  page,
}) => {
  await page.goto('/?scenario=older');
  const paused = page.getByRole('region', { name: 'Omarchy Settings Paused' });
  await expect(
    paused.getByText('This computer has Omarchy 4.0.6-1; Desktop has Omarchy 5.1.0.'),
  ).toBeVisible();
  await paused.getByRole('button', { name: 'Try Their Settings Anyway…' }).click();
  // Nothing changes until the person has read how to go back.
  await expect(paused.getByText("This computer's Omarchy settings are saved first.")).toBeVisible();
  expect(await calls(page, 'try_omarchy')).toEqual([]);
  expect(await a11y(page)).toEqual([]);
  await paused.getByRole('button', { name: 'Try Them' }).click();
  expect(await calls(page, 'try_omarchy')).toHaveLength(1);
  const trying = page.getByRole('region', { name: 'Trying Newer Omarchy Settings' });
  await trying.getByRole('button', { name: 'Put Back My Settings' }).click();
  expect(await calls(page, 'put_back_omarchy')).toHaveLength(1);
  await expect(page.getByRole('region', { name: 'Omarchy Settings Paused' })).toBeVisible();
});

test('the newer computer says which one to update', async ({ page }) => {
  await page.goto('/?scenario=newer');
  const older = page.getByRole('region', { name: 'Older Omarchy Elsewhere' });
  await expect(older.getByText('Omarchy 4.0.6-1.', { exact: false })).toBeVisible();
  await expect(
    older.getByText('Update the older computers, not this one', { exact: false }),
  ).toBeVisible();
});

test('settings shows the version and installs a newer release after a clear confirmation', async ({
  page,
}) => {
  await page.goto('/?scenario=update');
  await expect(page.getByText('Sign in to the same GitHub account')).toBeVisible();
  await page.keyboard.press('Control+8');
  const updates = page.getByRole('region', { name: 'Updates' });
  await expect(updates.getByText('Omarchy Sync 0.1.1')).toBeVisible();
  await updates.getByRole('button', { name: 'Check for Updates' }).click();
  await expect(updates.getByText('0.2.0 is available.')).toBeVisible();
  await updates.getByRole('button', { name: 'Update to 0.2.0…' }).click();
  await expect(updates.getByText('restarts Omarchy Sync', { exact: false })).toBeVisible();
  expect(await calls(page, 'install_update')).toEqual([]);
  expect(await a11y(page)).toEqual([]);
  await updates.getByRole('button', { name: 'Update', exact: true }).click();
  expect(await calls(page, 'install_update')).toEqual([
    expect.objectContaining({ args: expect.objectContaining({ version: '0.2.0' }) }),
  ]);
  // The update's steps and the download's percentage show in the Updates group itself.
  await expect(updates.getByText('Downloading Omarchy Sync 0.2.0 62%')).toBeVisible();
  await expect(updates.getByText('Restarting Omarchy Sync')).toBeVisible();
  expect(await a11y(page)).toEqual([]);
});

test('a copy built from source says how it updates instead', async ({ page }) => {
  await page.goto('/?scenario=source');
  await expect(page.getByText('Sign in to the same GitHub account')).toBeVisible();
  await page.keyboard.press('Control+8');
  const updates = page.getByRole('region', { name: 'Updates' });
  await expect(updates.getByText('Built from source', { exact: true })).toBeVisible();
  await expect(updates.getByRole('button', { name: 'Check for Updates' })).toHaveCount(0);
});

test('checking for updates says so when this copy is the latest', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await expect(page.getByText('Sign in to the same GitHub account')).toBeVisible();
  await page.keyboard.press('Control+8');
  const updates = page.getByRole('region', { name: 'Updates' });
  await updates.getByRole('button', { name: 'Check for Updates' }).click();
  await expect(updates.getByText('You have the latest version.')).toBeVisible();
  await expect(page.locator('.toast')).toHaveText('You have the latest version');
  await expect(updates.getByRole('button', { name: 'Check Again' })).toBeVisible();
  expect(await calls(page, 'check_update')).toHaveLength(1);
});
