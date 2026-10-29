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
  const status = page.getByRole('status').filter({ hasText: 'All your computers are in sync.' });
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
    ['4', 'macOS Theme'],
    ['5', 'Repositories'],
    ['6', 'Installed Apps'],
    ['7', 'App Data'],
    ['8', 'History'],
    ['9', 'Settings'],
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

test('the macOS theme applies the switches and sliders that changed', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'macOS Theme' }).click();
  const apply = page.getByRole('button', { name: 'Apply' });
  // Apply shows up once there is something to apply.
  await expect(apply).toBeHidden();
  // The current theme can't be removed from here.
  await expect(page.getByRole('switch', { name: 'Theme', exact: true })).toBeDisabled();
  for (const name of ['Dock', 'macOS Icons']) {
    await page.getByRole('switch', { name }).focus();
    await page.keyboard.press('Space');
  }
  // A setting waits for the piece it adjusts.
  await expect(page.getByRole('slider', { name: 'Dock Magnification' })).toBeDisabled();
  const glass = page.getByRole('slider', { name: 'Window Glass' });
  await glass.focus();
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('ArrowRight');
  await expect(glass).toHaveAttribute('aria-valuetext', '85%');
  await expect(
    page.getByText('Installs macOS Icons. Removes Dock. Sets Window Glass to 85%.'),
  ).toBeVisible();
  await apply.click();
  await expect(page.getByText('Done.')).toBeVisible();
  const [call] = await calls(page, 'macos_theme_apply');
  expect(call.args.add).toEqual(['icons']);
  expect(call.args.remove).toEqual(['dock']);
  expect(call.args.set).toEqual([['glass', 0.85]]);
  await expect(page.getByRole('switch', { name: 'Dock' })).not.toBeChecked();
  await expect(apply).toBeHidden();
});

test('switching an app off saves the choice', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'Installed Apps' }).click();
  const yazi = page.getByRole('switch', { name: 'Sync yazi (all computers)' });
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
    { cmd: 'set_app', args: { app: 'yazi', on: false } },
  ]);
  await expect(
    page.getByText('Your other computers get this the next time you sync.', { exact: false }),
  ).toBeVisible();
  // The Sync page counts the changed choices straight away, one row each, and says what.
  await page.keyboard.press('Control+1');
  await expect(page.getByText("2 changes on this computer aren't synced yet.")).toBeVisible();
  await expect(
    page.getByText('yazi switched off for this computer', { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText('Kitty terminal left out on this computer', { exact: true }),
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
  await page.keyboard.press('Control+9');
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
  // Each one can be left out of this Add; with none left there's nothing to add.
  await flip(page, 'Add Webcam Controls');
  await expect(waiting.getByRole('button', { name: 'Add 0 Plugins' })).toBeDisabled();
  await flip(page, 'Add Webcam Controls');
  await waiting.getByRole('button', { name: 'Add', exact: true }).click();
  expect(await calls(page, 'install_plugins')).toEqual([
    { cmd: 'install_plugins', args: { only: null } },
  ]);
  await expect(waiting).toBeHidden();
});

test('waiting apps are installed one by one, or left out on this computer', async ({ page }) => {
  await page.goto('/?scenario=waiting');
  const waiting = page.getByRole('region', { name: 'Apps Waiting to Install' });
  await expect(waiting.getByText('obsidian', { exact: true })).toBeVisible();
  expect(await a11y(page)).toEqual([]);
  await flip(page, 'Install vlc');
  await waiting.getByRole('button', { name: 'Install 1 App' }).click();
  expect(await calls(page, 'install_apps')).toEqual([
    { cmd: 'install_apps', args: { only: ['obsidian'] } },
  ]);
  await expect(waiting.getByText('obsidian', { exact: true })).toBeHidden();
  await waiting.getByRole('button', { name: "Don't Sync on This Computer" }).click();
  expect(await calls(page, 'app_here')).toEqual([
    { cmd: 'app_here', args: { app: 'vlc', on: false } },
  ]);
  await expect(waiting).toBeHidden();
});

test('installed apps come first, with one switch each; the rest wait below', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'Installed Apps' }).click();
  const mine = page.getByRole('region', { name: 'Your Apps' });
  const missing = page.getByRole('region', { name: 'Not Installed on This Computer' });
  const names = (group: typeof mine) => group.locator('.row > .text > div:first-child');
  await expect(names(mine)).toHaveText([/^brave-bin/, /^spotify/, /^tapecalc/, /^yazi/]);
  await expect(names(missing)).toHaveText([/^quicknote/]);
  await expect(page.getByText('Only on this computer').first()).toBeVisible();
  // Each app says where it comes from; the AUR is the last resort.
  await expect(page.locator('.source.aur')).toHaveText('AUR');
  await expect(page.locator('.source.omarchy')).toHaveText('Omarchy');
  expect(await a11y(page)).toEqual([]);
  // One switch reaches every computer.
  const spotify = page.getByRole('switch', { name: 'Sync spotify (all computers)' });
  await expect(spotify).not.toBeChecked();
  await flip(page, 'Sync spotify (all computers)');
  await expect(spotify).toBeChecked();
  const banner = page.getByText('Your other computers get this the next time you sync.', {
    exact: false,
  });
  await expect(banner).toBeVisible();
  await page.keyboard.press('Space');
  await expect(spotify).not.toBeChecked();
  await expect(banner).toBeHidden();
  expect(await calls(page, 'set_app')).toEqual([
    { cmd: 'set_app', args: { app: 'spotify', on: true } },
    { cmd: 'set_app', args: { app: 'spotify', on: false } },
  ]);
  // Like Configurations, this computer alone can be left out, and taken back.
  const brave = mine.locator('.row', { hasText: 'brave-bin' });
  await expect(brave.getByText('On for all your computers, this one included.')).toBeVisible();
  await brave.getByRole('button', { name: "Don't Sync on This Computer" }).click();
  await expect(brave.getByText('Not synced on this computer', { exact: false })).toBeVisible();
  await brave.getByRole('button', { name: 'Sync on This Computer', exact: true }).click();
  expect(await calls(page, 'app_here')).toEqual([
    { cmd: 'app_here', args: { app: 'brave-bin', on: false } },
    { cmd: 'app_here', args: { app: 'brave-bin', on: true } },
  ]);
  // One with install steps goes to the Sync page, which warns before running them.
  await missing.getByRole('button', { name: 'Install…', exact: true }).click();
  await expect(
    page.getByRole('region', { name: 'Apps to Install with Their Steps' }),
  ).toBeVisible();
  expect(await calls(page, 'install_with_script')).toEqual([]);
});

test('one click installs an app that is only on other computers, and it moves up', async ({
  page,
}) => {
  await page.goto('/?scenario=waiting');
  await page.getByRole('tab', { name: 'Installed Apps' }).click();
  const mine = page.getByRole('region', { name: 'Your Apps' });
  const missing = page.getByRole('region', { name: 'Not Installed on This Computer' });
  await missing
    .locator('.row', { hasText: 'obsidian' })
    .getByRole('button', { name: 'Install', exact: true })
    .click();
  expect(await calls(page, 'install_apps')).toEqual([
    { cmd: 'install_apps', args: { only: ['obsidian'] } },
  ]);
  await expect(mine.getByText('obsidian', { exact: true })).toBeVisible();
  await expect(missing.getByText('obsidian', { exact: true })).toHaveCount(0);
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

test('a plugin has one switch; one not here waits below with Install', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'Plugins' }).click();
  const mine = page.getByRole('region', { name: 'Your Plugins' });
  const missing = page.getByRole('region', { name: 'Not Installed on This Computer' });
  await expect(missing.getByText('Webcam Controls')).toBeVisible();
  await expect(mine.getByText('Webcam Controls')).toHaveCount(0);
  // No Enabled switch beside Sync for your own plugins: one switch a row.
  await expect(page.getByRole('switch', { name: 'Timezones enabled' })).toHaveCount(0);
  await flip(page, 'Sync Webcam Controls (all computers)');
  expect(await calls(page, 'set_plugin')).toEqual([
    { cmd: 'set_plugin', args: { plugin: 'webcam', on: false } },
  ]);
  await flip(page, 'Sync Webcam Controls (all computers)');
  await mine.getByRole('button', { name: "Don't Sync on This Computer" }).click();
  expect(await calls(page, 'plugin_here')).toEqual([
    { cmd: 'plugin_here', args: { plugin: 'timezones', on: false } },
  ]);
  await missing.getByRole('button', { name: 'Install' }).click();
  expect(await calls(page, 'install_plugins')).toEqual([
    { cmd: 'install_plugins', args: { only: ['webcam'] } },
  ]);
  await expect(missing).toBeHidden();
  await expect(mine.getByText('Webcam Controls')).toBeVisible();
});

test('plugins are switched on, added from git, updated and removed everywhere', async ({
  page,
}) => {
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
  await expect(page.getByRole('button', { name: 'Open github.com (Timezones)' })).toBeVisible();
  // Its source opens in the browser; Rust turns the git address into the page.
  await page.getByRole('button', { name: 'Open github.com (Timezones)' }).click();
  expect(await calls(page, 'open_page')).toEqual([
    {
      cmd: 'open_page',
      args: { kind: 'plugin', name: 'https://github.com/sspaeti/omarchy-plugin-timezones.git' },
    },
  ]);
  // The day's check found an update: Update stands out; updating marks it up to date.
  await page.getByRole('button', { name: /^Update \(an update is available/ }).click();
  await expect(page.getByText('Timezones is up to date.')).toBeVisible();
  await expect(page.getByText('Up to date', { exact: true })).toBeVisible();
  // Omarchy's own plugins link to their folder in Omarchy's repository.
  await expect(page.getByRole('button', { name: 'Open github.com (Clock)' })).toBeVisible();
  expect(await calls(page, 'update_plugins')).toEqual([
    expect.objectContaining({ args: expect.objectContaining({ id: 'timezones' }) }),
  ]);
  // Update All, at the top, goes plugin by plugin and sums up; a skipped plugin makes it a
  // warning.
  await page.getByRole('button', { name: 'Update All' }).click();
  const summary = page.getByText(/^Updated 1 plugin; the rest were up to date\. Webcam Controls/);
  await expect(summary).toBeVisible();
  await expect(page.locator('output.notice.warn')).toContainText("so it wasn't updated.");
  expect(await calls(page, 'update_plugins')).toEqual([
    expect.objectContaining({ args: expect.objectContaining({ id: 'timezones' }) }),
    expect.objectContaining({ args: expect.objectContaining({ id: null }) }),
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
  // Removing a synced plugin asks first and says it goes from every computer.
  await expect(
    page.getByText("choose Don't Sync on This Computer there first", { exact: false }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Remove… (Timezones)' }).click();
  await expect(
    page.getByText('Removes Timezones from every computer at its next sync.'),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Remove from Every Computer' }).click();
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
  // Its switch stays locked until it has one.
  const sync = page.getByRole('switch', { name: 'Sync tapecalc (all computers)' });
  await expect(sync).toBeDisabled();

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
  await expect(sync).toBeEnabled();
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
  await page.keyboard.press('Control+9');
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
  // Until then the app can't be installed here, and its switch is locked.
  await page.getByRole('tab', { name: 'Installed Apps' }).click();
  await expect(
    page.getByText('Needs the openai-chatgpt repository on this computer first'),
  ).toBeVisible();
  await expect(
    page.getByRole('switch', { name: 'Sync chatgpt-bin (all computers)' }),
  ).toBeDisabled();
  const missing = page.getByRole('region', { name: 'Not Installed on This Computer' });
  await expect(
    missing.locator('.row', { hasText: 'chatgpt-bin' }).getByRole('button', { name: 'Install' }),
  ).toHaveCount(0);
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
  await page.keyboard.press('Control+9');
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
  await page.keyboard.press('Control+9');
  const updates = page.getByRole('region', { name: 'Updates' });
  await expect(updates.getByText('Built from source', { exact: true })).toBeVisible();
  await expect(updates.getByRole('button', { name: 'Check for Updates' })).toHaveCount(0);
});

test('checking for updates says so when this copy is the latest', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await expect(page.getByText('Sign in to the same GitHub account')).toBeVisible();
  await page.keyboard.press('Control+9');
  const updates = page.getByRole('region', { name: 'Updates' });
  await updates.getByRole('button', { name: 'Check for Updates' }).click();
  await expect(updates.getByText('You have the latest version.')).toBeVisible();
  await expect(page.locator('.toast')).toHaveText('You have the latest version');
  await expect(updates.getByRole('button', { name: 'Check Again' })).toBeVisible();
  expect(await calls(page, 'check_update')).toHaveLength(1);
});

test('app data shows settings folders for apps that are installed or synced', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await expect(page.getByText('Sign in to the same GitHub account')).toBeVisible();
  await page.keyboard.press('Control+7');
  // Ghostty's folder syncs though Ghostty isn't installed here, and says so; Neovim's folder
  // neither syncs nor belongs to an installed or synced app, so it's left out.
  await expect(page.getByText('~/.config/ghostty, not installed on this computer')).toBeVisible();
  await expect(page.getByText('Neovim editor')).toHaveCount(0);
  expect(await a11y(page)).toEqual([]);
});

/** Flips a switch the way a keyboard does (its input is visually hidden). */
async function flip(page: Page, name: string) {
  await page.getByRole('switch', { name, exact: true }).focus();
  await page.keyboard.press('Space');
}

test('a first computer chooses what syncs before starting', async ({ page }) => {
  await page.goto('/?scenario=first');
  // Ghostty isn't installed, so it isn't promised, even though its folder is here.
  await expect(page.getByText('Your other Omarchy settings', { exact: false })).toBeVisible();
  await expect(page.getByText('ghostty terminal', { exact: false })).toBeHidden();
  await page.getByRole('button', { name: 'Choose What Syncs…' }).click();
  await flip(page, 'Sync Neovim editor');
  await flip(page, 'Sync System monitor');
  await flip(page, 'Sync Night light');
  await expect(page.getByText('neovim editor, system monitor.', { exact: false })).toBeVisible();
  expect(await a11y(page)).toEqual([]);
  await page.getByRole('button', { name: 'Start Syncing' }).click();
  const [start] = await calls(page, 'start');
  const folders = start.args.folders as string[];
  expect(folders).toContain('.config/nvim');
  expect(folders).toContain('.config/btop');
  expect(folders).not.toContain('.config/hypr/hyprsunset.conf');
  expect(folders).not.toContain('.config/ghostty');
  // No app unless switched on, every plugin unless switched off.
  expect(start.args.apps).toBeNull();
  expect(start.args.plugins).toBeNull();
});

test('a first computer picks its apps and plugins', async ({ page }) => {
  await page.goto('/?scenario=first');
  await page.getByRole('button', { name: 'Choose What Syncs…' }).click();
  await expect(page.getByRole('switch', { name: 'Sync yazi', exact: true })).not.toBeChecked();
  await flip(page, 'Sync yazi');
  await flip(page, 'Sync Timezones');
  expect(await a11y(page)).toEqual([]);
  await page.getByRole('button', { name: 'Start Syncing' }).click();
  const [start] = await calls(page, 'start');
  expect(start.args.apps).toEqual(['yazi']);
  expect(start.args.plugins).toEqual([]);
});

test('a joining computer keeps a folder separate', async ({ page }) => {
  await page.goto('/?scenario=joining');
  await page.getByRole('button', { name: 'Choose What Syncs…' }).click();
  await flip(page, 'Sync Neovim editor on this computer');
  await expect(page.getByText('Keep separate on this computer')).toBeVisible();
  await page.getByRole('button', { name: 'Use My Synced Settings' }).click();
  const [join] = await calls(page, 'join');
  expect(join.args.skipFolders).toEqual(['.config/nvim']);
});

test('a joining computer leaves out a synced app and plugin', async ({ page }) => {
  await page.goto('/?scenario=joining');
  await page.getByRole('button', { name: 'Choose What Syncs…' }).click();
  await flip(page, 'Sync yazi on this computer');
  await flip(page, 'Sync Timezones on this computer');
  await expect(page.getByText('Not offered on this computer')).toBeVisible();
  await expect(page.getByText('Not added on this computer')).toBeVisible();
  expect(await a11y(page)).toEqual([]);
  await page.getByRole('button', { name: 'Use My Synced Settings' }).click();
  const [join] = await calls(page, 'join');
  expect(join.args.skipApps).toEqual(['yazi']);
  expect(join.args.skipPlugins).toEqual(['timezones']);
});

test('signing in moves on by itself, and says so when it is not done', async ({ page }) => {
  await page.goto('/?scenario=signin');
  await page.getByRole('button', { name: 'Sign In to GitHub' }).click();
  await expect(page.getByText('one-time code', { exact: false })).toBeVisible();
  await page.getByRole('button', { name: "I've Signed In" }).click();
  await expect(page.getByText("GitHub sign-in isn't finished yet", { exact: false })).toBeVisible();
  // Looked at again every 2 seconds without being asked.
  const before = (await calls(page, 'probe')).length;
  await expect.poll(async () => (await calls(page, 'probe')).length).toBeGreaterThan(before);
});

test('a safety backup is restored after asking', async ({ page }) => {
  await page.goto('/?scenario=synced');
  await page.getByRole('tab', { name: 'History' }).click();
  await expect(page.getByText('Safety Backups')).toBeVisible();
  await page.getByRole('button', { name: /^Restore Before first sync/ }).click();
  expect(await a11y(page)).toEqual([]);
  await page.getByRole('button', { name: 'Restore Backup' }).click();
  await expect(page.locator('.toast').getByText('Restored')).toBeVisible();
  const [restore] = await calls(page, 'restore_backup');
  expect(restore.args.name).toMatch(/^before-first-sync-\d+\.tar\.gz$/);
});

test('a clash on several files can be settled for all at once', async ({ page }) => {
  await page.goto('/?scenario=conflict');
  await page.getByRole('button', { name: 'Sync Now' }).click();
  await page.getByRole('button', { name: 'Use Desktop for All' }).click();
  await page.getByRole('button', { name: 'Keep Chosen Versions' }).click();
  const [resolve] = await calls(page, 'resolve_conflict');
  expect(resolve.args.choices).toEqual({
    'dot_config/hypr/bindings.lua': 'Theirs',
    'dot_config/ghostty/config': 'Theirs',
  });
});
