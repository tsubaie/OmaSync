// Stands in for the Rust side when the page runs in a plain browser for the end-to-end tests
// (`vite --mode e2e`). Pick a scenario with ?scenario=setup|synced|conflict|syncing|checking. Every call is
// recorded on window.e2eCalls so tests can check what the page asked for.

import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import type { Catalog, Computer, Data, Problem, SetupState, Status } from './api';

declare global {
  interface Window {
    e2eCalls: { cmd: string; args: Record<string, unknown> }[];
  }
}

const scenario = new URLSearchParams(location.search).get('scenario') ?? 'synced';
// ?theme=light previews a light Omarchy theme (Catppuccin Latte); the default is the dark fallback.
// Computers beyond Laptop and Desktop.
const extra = Math.max(
  0,
  Number(new URLSearchParams(location.search).get('computers') ?? (scenario === 'many' ? 6 : 2)) -
    2,
);
const theme = new URLSearchParams(location.search).get('theme');
const LATTE = [
  'background = "#eff1f5"',
  'dark_background = "#e3e4e8"',
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

// The same folders as FOLDERS in sync.rs: [path, name, synced by default].
const FOLDERS: [string, string, boolean][] = [
  ['.config/omarchy', 'Other Omarchy settings', true],
  ['.config/omarchy/themes', 'Current theme', true],
  ['.config/omarchy/extensions', 'Omarchy menu additions', true],
  ['.config/omarchy/hooks', 'Hooks', true],
  ['.config/omarchy/plugins', 'Plugins', true],
  ['.config/omarchy/shell.json', 'Bar layout and plugin settings', true],
  ['.config/hypr', 'Other window settings', true],
  ['.config/hypr/bindings.lua', 'Keyboard shortcuts', true],
  ['.config/hypr/input.lua', 'Keyboard, mouse and touchpad', true],
  ['.config/hypr/looknfeel.lua', 'Look and feel', true],
  ['.config/hypr/autostart.lua', 'Apps that start with Omarchy', true],
  ['.config/hypr/hyprsunset.conf', 'Night light', true],
  ['.local/share/applications', 'Web apps and their icons', true],
  ['.config/ghostty', 'Ghostty terminal', true],
  ['.config/nvim', 'Neovim editor', false],
  ['.config/btop', 'System monitor', false],
];
const catalog: Catalog = {
  data_label: 'Apps and sync choices',
  folders: FOLDERS.map(([path, name, on]) => ({ path, name, default: on, exists: true })),
};

let data: Data = {
  folders: FOLDERS.filter(([, , on]) => on).map(([path]) => path),
  apps: ['brave-bin', 'yazi', 'quicknote', ...(scenario === 'repo' ? ['chatgpt-bin'] : [])],
  // On Desktop only, from outside the repos: it waits here for its install command.
  outside: { quicknote: { command: 'curl -fsSL https://example.com/quicknote.sh | bash' } },
  plugins: ['timezones', 'webcam'],
  computers: {
    // "older": this computer runs an older Omarchy, so its Omarchy settings are paused.
    // "newer": Desktop does.
    aaaa11112222: {
      name: 'Laptop',
      plugins: ['timezones'],
      omarchy: scenario === 'older' ? '4.0.6-1' : '5.1.0',
      paused: scenario === 'older',
    },
    bbbb33334444: {
      name: 'Desktop',
      skip_apps: ['yazi'],
      plugins: ['timezones', 'webcam'],
      omarchy: scenario === 'newer' ? '4.0.6-1' : '5.1.0',
      paused: scenario === 'newer',
    },
    ...Object.fromEntries(
      (
        [
          ['cccc55556666', { name: 'Work Laptop' }],
          ['dddd77778888', { name: 'Studio' }],
          ['eeee99990000', { name: 'Living Room' }],
          ['ffff11112222', { name: 'Travel' }],
        ] as [string, Computer][]
      ).slice(0, extra),
    ),
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

// On by default, like the real app.
let barIndicator = true;

const status = (): Status => ({
  host: 'aaaa11112222',
  sync_space: 'https://github.com/someone/omarchy-sync',
  schedule_error: null,
  bar_indicator: barIndicator,
  version: '0.1.1',
  installed_how: scenario === 'source' ? 'Source' : 'Package',
  data,
  overview: {
    unshared: scenario === 'conflict' ? ['.config/hypr/bindings.lua'] : [],
    incoming: 0,
    online: true,
    computers: [
      ['aaaa11112222', now - 120],
      ['bbbb33334444', now - 3600],
      // More computers: "many" is six; ?computers=3 to 6 picks how many in all.
      ...(
        [
          ['cccc55556666', now - 600],
          ['dddd77778888', now - 7200],
          ['eeee99990000', now - 86400],
          ['ffff11112222', now - 3 * 86400],
        ] as [string, number][]
      ).slice(0, extra),
    ],
    apps: {
      installed: ['brave-bin', 'yazi', 'tapecalc'],
      mine: ['brave-bin', 'yazi', 'spotify', 'tapecalc'],
      // ("repo": chatgpt-bin waits for its maker's repository.)
      // Installed from its own repository, not Arch's or the AUR.
      outside: ['tapecalc'],
      source: {
        'brave-bin': 'omarchy',
        yazi: 'arch',
        spotify: 'aur',
        tapecalc: 'elsewhere',
        quicknote: 'elsewhere',
      },
      about: { 'brave-bin': 'Web browser', spotify: 'Music', tapecalc: 'Calculation tape' },
    },
    incoming_files: [],
    incoming_from: [],
    // "repo": an app here comes from its maker's own repository, which this computer lacks.
    repos_here: [],
    missing_repos:
      scenario === 'repo'
        ? [
            {
              name: 'openai-chatgpt',
              repository: {
                servers: ['https://persistent.oaistatic.com/codex-app-prod/linux/arch/x86_64'],
                sig_level: ['PackageRequired', 'DatabaseRequired'],
                key: {
                  fingerprint: '3BFA0E4AE8B8CC16A2D9BA684A3B4A566C4660E4',
                  owner: 'Codex Linux Repository',
                  armored: '-----BEGIN PGP PUBLIC KEY BLOCK-----',
                },
              },
              apps: ['chatgpt-bin'],
            },
          ]
        : [],
    plugins: [
      { id: 'timezones', name: 'Timezones', description: 'World clock with an hour-grid popup.' },
      { id: 'webcam', name: 'Webcam Controls', description: 'Control any webcam from the bar.' },
    ],
    history: [
      {
        sha: 'a'.repeat(40),
        computer: 'bbbb33334444',
        when: now - 3600,
        labels: ['Keyboard shortcuts', 'Keyboard, mouse and touchpad'],
        is_undo: false,
        is_merge: false,
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
        is_merge: false,
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

// Two settings changed on both computers: keyboard shortcuts and the terminal.
const conflict: Problem = {
  kind: 'Conflict',
  files: ['.config/hypr/bindings.lua', '.config/ghostty/config'],
  by: ['bbbb33334444', now - 300],
  broken: [],
  versions: [
    {
      path: '.config/hypr/bindings.lua',
      source: 'dot_config/hypr/bindings.lua',
      mine: [
        'o.bind("SUPER + RETURN", "Terminal", "uwsm-app -- ghostty")',
        'o.bind("SUPER + B", "Browser", "omarchy-launch-browser")',
        'o.bind("SUPER + E", "Files", "uwsm-app -- nautilus")',
        'o.bind("SUPER + M", "Music", "uwsm-app -- spotify")',
      ].join('\n'),
      theirs: [
        'o.bind("SUPER + RETURN", "Terminal", "uwsm-app -- ghostty")',
        'o.bind("SUPER + B", "Browser", "omarchy-launch-browser --private")',
        'o.bind("SUPER + E", "Files", "uwsm-app -- nautilus")',
        'o.bind("SUPER + N", "Notes", "uwsm-app -- obsidian")',
      ].join('\n'),
      binary: false,
    },
    {
      path: '.config/ghostty/config',
      source: 'dot_config/ghostty/config',
      mine: 'font-size = 12\nwindow-padding-x = 14',
      theirs: 'font-size = 14\nwindow-padding-x = 14',
      binary: false,
    },
  ],
};

window.e2eCalls = [];
const SYNC_STEPS = [
  "Saving this computer's changes",
  'Saving changed settings',
  'Looking for new files',
  'Getting changes from your other computers',
  'Updating this computer',
  'Sharing with your other computers',
  'Checking your apps and plugins',
];

/** Sends progress lines to a page Channel, as the Rust side does. */
function emit(channel: unknown, lines: string[]) {
  const id = (channel as { id: number } | undefined)?.id;
  if (id === undefined) return;
  // Tauri's mock IPC delivers a message to a Channel by its id.
  const internals: { runCallback: (id: number, data: unknown) => void } = Reflect.get(
    window,
    '__TAURI_INTERNALS__',
  );
  lines.forEach((message, index) => internals.runCallback(id, { index, message }));
}

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
      // After a sync, its last step carries on the sync's progress.
      if (a.justSynced) emit(a.onStep, [SYNC_STEPS.at(-1)!]);
      // "checking" never finishes its first check, to show the app before there's anything to see.
      return scenario === 'checking' ? new Promise(() => {}) : status();
    case 'sync_now': {
      // Reports its steps like sync.rs. "syncing" stops partway, sharing at 62%, to show (and
      // photograph) the progress.
      emit(a.onStep, [
        `\u0001plan\t${SYNC_STEPS.join('\t')}`,
        ...SYNC_STEPS.slice(0, -1),
        ...(scenario === 'syncing' ? ['\u0001percent\t62'] : []),
      ]);
      if (scenario === 'syncing') return new Promise(() => {});
      return scenario === 'conflict' ? Promise.reject(conflict) : { notes: [], waiting: [] };
    }
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
    case 'resolve_conflict':
      return { notes: [], waiting: [] };
    case 'report':
    case 'add_repository':
    case 'update_omarchy':
      return null;
    case 'try_omarchy':
    case 'put_back_omarchy': {
      const trying = cmd === 'try_omarchy';
      const me = { ...data.computers.aaaa11112222, tried: trying ? 5 : 0, paused: !trying };
      data = { ...data, computers: { ...data.computers, aaaa11112222: me } };
      return trying ? data : null;
    }
    case 'set_bar_indicator':
      barIndicator = !!a.on;
      return null;
    case 'install_with_script':
      return null;
    case 'set_app_script':
      data = {
        ...data,
        outside: {
          ...data.outside,
          [a.app as string]: { command: a.command as string },
        },
      };
      return data;
    case 'remove_app_script':
      data = { ...data, outside: { ...data.outside, [a.app as string]: null } };
      return data;
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
    case 'check_update':
      return scenario === 'update' ? { version: '0.2.0' } : null;
    case 'install_update':
      emit(a.onStep, [
        "\u0001plan\tDownloading Omarchy Sync 0.2.0\tChecking it's genuine\tInstalling\tRestarting Omarchy Sync",
        'Downloading Omarchy Sync 0.2.0',
        "Checking it's genuine",
        'Installing',
        'Restarting Omarchy Sync',
      ]);
      return null;
    case 'open_release':
      return null;
    default:
      return null;
  }
});
