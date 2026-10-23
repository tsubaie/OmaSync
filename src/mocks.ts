// Stands in for the Rust side when the page runs in a plain browser for the end-to-end tests
// (`vite --mode e2e`). Pick a scenario with ?scenario=setup|synced|conflict. Every call is
// recorded on window.e2eCalls so tests can check what the page asked for.

import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import type { Catalog, Data, Problem, SetupState, Status } from './api';

declare global {
  interface Window {
    e2eCalls: { cmd: string; args: Record<string, unknown> }[];
  }
}

const scenario = new URLSearchParams(location.search).get('scenario') ?? 'synced';
// ?theme=light previews a light Omarchy theme (Catppuccin Latte); the default is the dark fallback.
const theme = new URLSearchParams(location.search).get('theme');
const LATTE = [
  'background = "#eff1f5"',
  'foreground = "#4c4f69"',
  'accent = "#1e66f5"',
  'lighter_background = "#ccd0da"',
  'dark_foreground = "#6c6f85"',
  'green = "#40a02b"',
  'yellow = "#df8e1d"',
  'red = "#d20f39"',
  'cyan = "#179299"',
  'orange = "#fe640b"',
  'magenta = "#ea76cb"',
].join('\n');
const now = Math.floor(Date.now() / 1000);

const catalog: Catalog = {
  data_label: 'Apps and sync choices',
  folders: [
    {
      path: '.config/omarchy',
      name: 'Themes, menus and hooks',
      default: true,
      exists: true,
    },
    { path: '.config/omarchy/plugins', name: 'Plugins', default: true, exists: true },
    {
      path: '.config/omarchy/shell.json',
      name: 'Bar layout and plugin settings',
      default: true,
      exists: true,
    },
    {
      path: '.config/hypr',
      name: 'Windows, keyboard shortcuts and input',
      default: true,
      exists: true,
    },
    { path: '.config/nvim', name: 'Neovim editor', default: false, exists: true },
  ],
};

let data: Data = {
  folders: [
    '.config/omarchy',
    '.config/omarchy/plugins',
    '.config/omarchy/shell.json',
    '.config/hypr',
  ],
  apps: ['brave-bin', 'yazi'],
  plugins: ['timezones', 'webcam'],
  computers: {
    aaaa11112222: { name: 'Laptop', plugins: ['timezones'] },
    bbbb33334444: { name: 'Desktop', skip_apps: ['yazi'], plugins: ['timezones', 'webcam'] },
  },
};

const shared = data;
const same = (d: Data) =>
  JSON.stringify(d, (_, v) => (Array.isArray(v) ? (v.length ? v.toSorted() : undefined) : v));

/** A copy of Data::place in sync.rs, enough for the tests. */
function place(
  d: Data,
  list: 'apps' | 'plugins',
  skip: 'skip_apps' | 'skip_plugins',
  computer: string,
  item: string,
  on: boolean,
  owners: string[],
): Data {
  const next: Data = structuredClone(d);
  const ids = Object.keys(next.computers);
  const wants = (id: string) =>
    !!next[list]?.includes(item) && !(next.computers[id][skip] ?? []).includes(item);
  const set = (id: string, yes: boolean) => {
    const rest = (next.computers[id][skip] ?? []).filter((x) => x !== item);
    next.computers[id][skip] = yes ? rest : [...rest, item];
  };
  if (on && !ids.some(wants)) {
    next[list] = [...(next[list] ?? []), item];
    for (const id of ids) set(id, id === computer || owners.includes(id));
  } else set(computer, on);
  if (!ids.some(wants)) {
    next[list] = (next[list] ?? []).filter((x) => x !== item);
    for (const id of ids) set(id, true);
  }
  return next;
}

const status = (): Status => ({
  host: 'aaaa11112222',
  sync_space: 'https://github.com/someone/omarchy-sync',
  schedule_error: null,
  data,
  overview: {
    unshared: scenario === 'conflict' ? ['.config/hypr/bindings.lua'] : [],
    incoming: 0,
    online: true,
    computers: [
      ['aaaa11112222', now - 120],
      ['bbbb33334444', now - 3600],
    ],
    apps: {
      installed: ['brave-bin', 'yazi'],
      mine: ['brave-bin', 'yazi', 'spotify'],
      about: { 'brave-bin': 'Web browser', spotify: 'Music' },
    },
    incoming_files: [],
    incoming_from: [],
    plugins: [
      { id: 'timezones', name: 'Timezones', description: 'World clock with an hour-grid popup.' },
      { id: 'webcam', name: 'Webcam Controls', description: 'Control any webcam from the bar.' },
    ],
    history: [
      {
        sha: 'a'.repeat(40),
        computer: 'bbbb33334444',
        when: now - 3600,
        labels: ['Windows, keyboard shortcuts and input'],
        is_undo: false,
        first: false,
        files: [
          { path: '.config/hypr/bindings.lua', how: 'Changed' },
          { path: '.config/hypr/input.lua', how: 'Added' },
        ],
      },
      {
        sha: 'b'.repeat(40),
        computer: 'aaaa11112222',
        when: now - 7200,
        labels: [],
        is_undo: false,
        first: true,
        files: [],
      },
    ],
  },
});

const setup: SetupState =
  scenario === 'setup'
    ? { missing: ['chezmoi'], signed_in: false, login: null, found: false, ready: false }
    : { missing: [], signed_in: true, login: 'someone', found: false, ready: true };

const conflict: Problem = {
  kind: 'Conflict',
  files: ['.config/hypr/bindings.lua'],
  by: ['bbbb33334444', now - 300],
};

window.e2eCalls = [];
mockWindows('main');
mockIPC((cmd, args) => {
  window.e2eCalls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
  const a = (args ?? {}) as Record<string, never>;
  switch (cmd) {
    case 'look':
      return { colors_toml: theme === 'light' ? LATTE : '', motion: false };
    case 'folders':
      return catalog;
    case 'probe':
      return setup;
    case 'overview':
      // "checking" never finishes its first check, to show the app before there's anything to see.
      return scenario === 'checking' ? new Promise(() => {}) : status();
    case 'sync_now':
      return scenario === 'conflict' && a.prefer === 'Ask'
        ? Promise.reject(conflict)
        : { notes: [], waiting: [] };
    case 'add_apps':
      data = { ...data, apps: [...new Set([...data.apps, ...(a.apps as string[])])] };
      return data;
    case 'choices_changed':
      // Like the Rust side: order and empty lists don't count.
      return same(data) !== same(shared);
    case 'install_plugins': {
      const laptop = data.computers.aaaa11112222;
      data = {
        ...data,
        computers: {
          ...data.computers,
          aaaa11112222: { ...laptop, plugins: ['timezones', 'webcam'] },
        },
      };
      return data;
    }
    case 'set_app':
      data = place(data, 'apps', 'skip_apps', a.computer as string, a.app as string, !!a.on, [
        'aaaa11112222',
      ]);
      return data;
    case 'set_plugin': {
      const owners = Object.keys(data.computers).filter((id) =>
        data.computers[id].plugins?.includes(a.plugin as string),
      );
      data = place(
        data,
        'plugins',
        'skip_plugins',
        a.computer as string,
        a.plugin as string,
        !!a.on,
        owners,
      );
      return data;
    }
    case 'rename_computer':
      data = {
        ...data,
        computers: {
          ...data.computers,
          aaaa11112222: { ...data.computers.aaaa11112222, name: a.name },
        },
      };
      return data;
    case 'set_auto':
    case 'set_install_auto':
      return data;
    default:
      return null;
  }
});
