// Typed calls into the Rust side (src-tauri/src/main.rs). The types mirror the Rust structs;
// keep them in step when a command changes.

import { Channel, invoke } from '@tauri-apps/api/core';

export type Look = { colors_toml: string; motion: boolean };
export type Folder = { path: string; name: string; default: boolean; exists: boolean };
export type Catalog = { folders: Folder[]; data_label: string };
export type Plugin = { id: string; name: string; description: string };

export type SetupState = {
  missing: string[];
  signed_in: boolean;
  login: string | null;
  found: boolean;
  ready: boolean;
};

export type Computer = {
  /** What people call this computer; keys in `Data.computers` are machine IDs. */
  name?: string;
  /** Older names this computer synced under. */
  aka?: string[];
  skip_apps?: string[];
  skip_files?: string[];
  /** Plugin IDs on this computer. */
  plugins?: string[];
  /** Synced plugins this computer doesn't want. */
  skip_plugins?: string[];
  manual?: boolean;
  install_without_asking?: boolean;
};
export type Data = {
  folders: string[];
  apps: string[];
  /** Plugin IDs shared between computers. */
  plugins?: string[];
  theme?: string;
  computers: Record<string, Computer>;
};

export type Entry = {
  sha: string;
  computer: string;
  when: number;
  labels: string[];
  is_undo: boolean;
  first: boolean;
};
export type Overview = {
  /** Paths under home changed here and not shared yet. */
  unshared: string[];
  incoming: number;
  online: boolean;
  /** [computer name, last sync as unix time], this computer first. */
  computers: [string, number | null][];
  apps: { installed: string[]; mine: string[]; about: Record<string, string> };
  incoming_files: string[];
  incoming_from: string[];
  history: Entry[];
  /** Every computer's plugins. */
  plugins: Plugin[];
};
export type Status = {
  overview: Overview;
  data: Data;
  /** This computer's ID. */
  host: string;
  /** The private GitHub repo everything syncs through, as a web address. */
  sync_space: string | null;
  schedule_error: string | null;
};

export type Report = { notes: string[]; waiting: string[] };
export type Conflict = { files: string[]; by: [string, number] | null };
/** What a failed command throws. */
export type Problem =
  { kind: 'Failed'; message: string; details: string } | ({ kind: 'Conflict' } & Conflict);

export type Prefer = 'Ask' | 'ThisComputer' | 'OtherComputer';
type OnStep = (step: string) => void;

const steps = (onStep: OnStep) => new Channel<string>(onStep);

export const api = {
  look: () => invoke<Look>('look'),
  folders: () => invoke<Catalog>('folders'),
  probe: () => invoke<SetupState>('probe'),
  overview: (onStep: OnStep) => invoke<Status>('overview', { onStep: steps(onStep) }),
  syncNow: (prefer: Prefer, onStep: OnStep) =>
    invoke<Report>('sync_now', { prefer, onStep: steps(onStep) }),
  undo: (sha: string, onStep: OnStep) => invoke<Report>('undo', { sha, onStep: steps(onStep) }),
  installHelpers: () => invoke<void>('install_helpers'),
  signIn: () => invoke<void>('sign_in'),
  start: (onStep: OnStep) => invoke<void>('start', { onStep: steps(onStep) }),
  /** Returns where this computer's previous settings were backed up. */
  join: (onStep: OnStep) => invoke<string>('join', { onStep: steps(onStep) }),
  installApps: () => invoke<void>('install_apps'),
  /** Whether the sync choices differ from what was last shared. */
  choicesChanged: () => invoke<boolean>('choices_changed'),
  setPluginSync: (plugin: string, on: boolean) => invoke<Data>('set_plugin_sync', { plugin, on }),
  setPlugin: (computer: string, plugin: string, on: boolean) =>
    invoke<Data>('set_plugin', { computer, plugin, on }),
  /** Adds the synced plugins this computer is waiting for. */
  installPlugins: () => invoke<Data>('install_plugins'),
  setApp: (computer: string, app: string, on: boolean) =>
    invoke<Data>('set_app', { computer, app, on }),
  stopApp: (app: string) => invoke<Data>('stop_app', { app }),
  addApps: (apps: string[]) => invoke<Data>('add_apps', { apps }),
  setFolder: (path: string, on: boolean) => invoke<Data>('set_folder', { path, on }),
  folderHere: (path: string, on: boolean) => invoke<Data>('folder_here', { path, on }),
  setAuto: (on: boolean) => invoke<Data>('set_auto', { on }),
  setInstallAuto: (on: boolean) => invoke<Data>('set_install_auto', { on }),
  renameComputer: (name: string) => invoke<Data>('rename_computer', { name }),
  forgetComputer: (id: string) => invoke<Data>('forget_computer', { id }),
  openSyncSpace: () => invoke<void>('open_sync_space'),
  /** Returns where the local sync folder was moved. */
  disconnect: () => invoke<string>('disconnect'),
};
