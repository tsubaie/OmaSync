// Several computers syncing through the real app, end to end. Each "computer" is its own home
// folder with its own computer ID (OMARCHY_SYNC_HOST); GitHub is a bare repository on disk
// that git reaches at its GitHub address (url.insteadOf); system tools are fakes that only
// record what they're asked (e2e/multi/fakes). The real app window is driven through
// tauri-driver and WebKit's WebDriver, one computer at a time.
//
// Runs in a container: `pnpm test:multi` (see e2e/multi/run.sh).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createConnection } from 'node:net';
import { dirname, join, resolve } from 'node:path';
import { remote } from 'webdriverio';

const root = '/tmp/omasync-e2e';
const github = join(root, 'github.git');
const fakes = resolve('e2e/multi/fakes');
const app = process.env.OMASYNC_BIN;
rmSync(root, { recursive: true, force: true });

/** A computer: its own home folder and ID, with the fakes first on its PATH. */
function computer(id) {
  const home = join(root, id);
  const env = {
    ...process.env,
    HOME: home,
    XDG_CONFIG_HOME: join(home, '.config'),
    XDG_DATA_HOME: join(home, '.local/share'),
    XDG_STATE_HOME: join(home, '.local/state'),
    XDG_CACHE_HOME: join(home, '.cache'),
    OMARCHY_SYNC_HOST: id,
    E2E_REMOTE: github,
    PATH: `${fakes}:${process.env.PATH}`,
  };
  mkdirSync(join(home, '.config/hypr'), { recursive: true });
  mkdirSync(join(home, '.e2e'), { recursive: true });
  execFileSync(
    'git',
    ['config', '--global', `url.${github}.insteadOf`, 'https://github.com/tester/omarchy-sync.git'],
    { env },
  );
  const path = (p) => join(home, p);
  return {
    id,
    env,
    write(p, text) {
      mkdirSync(dirname(path(p)), { recursive: true });
      writeFileSync(path(p), text);
    },
    read: (p) => readFileSync(path(p), 'utf8'),
    has: (p) => existsSync(path(p)),
    list: (p) => (existsSync(path(p)) ? readdirSync(path(p)) : []),
  };
}

const portOpen = (port) =>
  new Promise((done) => {
    const socket = createConnection(port, '127.0.0.1');
    socket.on('connect', () => (socket.destroy(), done(true)));
    socket.on('error', () => done(false));
  });

/** Opens the app as this computer, runs `steps` in its window, then closes it. */
async function use(c, steps) {
  const driver = spawn('tauri-driver', [], { env: c.env, stdio: 'ignore' });
  for (let i = 0; i < 100 && !(await portOpen(4444)); i++)
    await new Promise((r) => setTimeout(r, 100));
  const w = await remote({
    hostname: '127.0.0.1',
    port: 4444,
    logLevel: 'error',
    capabilities: { 'tauri:options': { application: app } },
  });
  try {
    await steps(w);
  } finally {
    await w.deleteSession().catch(() => {});
    driver.kill();
    for (let i = 0; i < 50 && (await portOpen(4444)); i++)
      await new Promise((r) => setTimeout(r, 100));
  }
}

const press = async (w, text) => {
  const b = await w.$(`button=${text}`);
  await b.waitForClickable({ timeout: 30_000 });
  await b.click();
};
const see = (w, text, timeout = 60_000) =>
  w.$(`//*[normalize-space()="${text}"]`).waitForDisplayed({ timeout });
const inStep = (w) => see(w, 'Everything matches your other computers.');

const lua = (text) => `o.bind("SUPER + RETURN", "Terminal", "${text}")\n`;
const keys = '.config/hypr/bindings.lua';
const a = computer('aaaa00000001');
const b = computer('bbbb00000002');

test('the first computer sets up and saves its settings', async () => {
  a.write(keys, lua('ghostty'));
  await use(a, async (w) => {
    await press(w, 'Start Syncing');
    await inStep(w);
  });
  const saved = execFileSync('git', ['-C', github, 'show', 'HEAD:dot_config/hypr/bindings.lua']);
  assert.equal(String(saved), lua('ghostty'));
});

test('a second computer joins, backs up its own settings, and gets the synced ones', async () => {
  b.write(keys, lua('kitty'));
  await use(b, async (w) => {
    await press(w, 'Use My Synced Settings');
    await inStep(w);
    // Omarchy names every computer "omarchy", so this one gets a number, and says so.
    await see(
      w,
      'Another computer is already called omarchy, so this one is omarchy 2. You can rename it in Settings.',
    );
  });
  assert.equal(b.read(keys), lua('ghostty'));
  assert.equal(b.list('.local/share/omarchy-sync/backups').length, 1);
});

test('a change on one computer reaches the other', async () => {
  a.write(keys, lua('alacritty'));
  await use(a, async (w) => {
    await press(w, 'Sync Now');
    await inStep(w);
  });
  await use(b, async (w) => {
    await press(w, 'Sync Now');
    await inStep(w);
  });
  assert.equal(b.read(keys), lua('alacritty'));
});

test('a clash shows both versions and keeps the one chosen', async () => {
  a.write(keys, lua('foot'));
  await use(a, async (w) => {
    await press(w, 'Sync Now');
    await inStep(w);
  });
  b.write(keys, lua('wezterm'));
  await use(b, async (w) => {
    await press(w, 'Sync Now');
    await see(w, 'The same setting was changed on two computers.');
    // Both versions, side by side, before choosing.
    await see(w, lua('foot').trim());
    await see(w, lua('wezterm').trim());
    await (await w.$('//label[normalize-space()="Keep omarchy\'s Version"]')).click();
    await press(w, 'Keep Chosen Versions');
    await inStep(w);
  });
  assert.equal(b.read(keys), lua('foot'));
  await use(a, async (w) => {
    await press(w, 'Sync Now');
    await inStep(w);
  });
  assert.equal(a.read(keys), lua('foot'));
});

test('six computers in step, shown together', async () => {
  for (const id of ['cccc00000003', 'dddd00000004', 'eeee00000005', 'ffff00000006']) {
    await use(computer(id), async (w) => {
      await press(w, 'Use My Synced Settings');
      await inStep(w);
    });
  }
  await use(a, async (w) => {
    await press(w, 'Sync Now');
    await inStep(w);
    await see(w, '5 Other Computers');
  });
});
