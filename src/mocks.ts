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
const now = Math.floor(Date.now() / 1000);

const catalog: Catalog = {
  data_label: 'Apps and sync choices',
  folders: [
    {
      path: '.config/omarchy',
      name: 'Omarchy settings, themes and plugins',
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
  folders: ['.config/omarchy', '.config/hypr'],
  apps: ['brave-bin', 'yazi'],
  computers: {
    aaaa11112222: { name: 'Laptop' },
    bbbb33334444: { name: 'Desktop', skip_apps: ['yazi'] },
  },
};

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
      local_only: ['spotify'],
      about: { 'brave-bin': 'Web browser', spotify: 'Music' },
    },
    incoming_files: [],
    incoming_from: [],
    history: [
      {
        sha: 'a'.repeat(40),
        computer: 'bbbb33334444',
        when: now - 3600,
        labels: ['Windows, keyboard shortcuts and input'],
        is_undo: false,
        first: false,
      },
      {
        sha: 'b'.repeat(40),
        computer: 'aaaa11112222',
        when: now - 7200,
        labels: [],
        is_undo: false,
        first: true,
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
      return { colors_toml: '', motion: false, blur: false };
    case 'folders':
      return catalog;
    case 'probe':
      return setup;
    case 'overview':
      return status();
    case 'sync_now':
      return scenario === 'conflict' && a.prefer === 'Ask'
        ? Promise.reject(conflict)
        : { notes: [], waiting: [] };
    case 'set_app': {
      const skip = new Set(data.computers[a.computer]?.skip_apps ?? []);
      if (a.on) skip.delete(a.app);
      else skip.add(a.app);
      data = {
        ...data,
        computers: {
          ...data.computers,
          [a.computer]: { ...data.computers[a.computer], skip_apps: [...skip] },
        },
      };
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
