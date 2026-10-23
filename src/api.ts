// Typed calls into the Rust side (src-tauri/src/main.rs). The types mirror the Rust structs;
// keep them in step when a command changes.

import { Channel, invoke } from '@tauri-apps/api/core';

export type Look = { colors_toml: string; motion: boolean };
export type Folder = { path: string; name: string; default: boolean; exists: boolean };
export type Catalog = { folders: Folder[]; data_label: string };
export type Plugin = { id: string; name: string; description: string };
/** An extra package repository another computer has, and the apps here that need it. */
export type MissingRepo = {
  name: string;
  repository: {
    servers: string[];
    sig_level: string[];
    key?: { fingerprint: string; owner: string; armored: string };
  };
  apps: string[];
};
/** How to install an app from outside the repos: a command, run in an empty folder. */
export type Script = { command: string };

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
  /** Its Omarchy version, as `omarchy version` prints it. */
  omarchy?: string;
  /** Omarchy's own settings don't sync there for now: a newer Omarchy runs elsewhere. */
  paused?: boolean;
  /** The newer Omarchy major version whose settings it's trying anyway. */
  tried?: number;
};
export type Data = {
  folders: string[];
  apps: string[];
  /** Apps from outside Arch's repos and the AUR, with their install script (null: none yet). */
  outside?: Record<string, Script | null>;
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
  /** Where two computers' changes were joined after a clash; it can't be undone on its own. */
  is_merge: boolean;
  first: boolean;
  /** Every file it changed: what Undo puts back. `path` is under home, or the data label. */
  files: { path: string; how: 'Added' | 'Changed' | 'Removed' }[];
};
export type Overview = {
  /** Paths under home changed here and not shared yet. */
  unshared: string[];
  incoming: number;
  online: boolean;
  /** [computer name, last sync as unix time], this computer first. */
  computers: [string, number | null][];
  apps: {
    installed: string[];
    mine: string[];
    /** Those of `mine` from neither Arch's repos nor the AUR. */
    outside: string[];
    /** Where each app comes from: omarchy, arch, aur, elsewhere, or another repository's name. */
    source: Record<string, string>;
    about: Record<string, string>;
  };
  incoming_files: string[];
  incoming_from: string[];
  history: Entry[];
  /** Every computer's plugins. */
  plugins: Plugin[];
  /** Extra repositories this computer needs for apps it should have. */
  missing_repos: MissingRepo[];
  repos_here: MissingRepo[];
};
export type Status = {
  overview: Overview;
  data: Data;
  /** This computer's ID. */
  host: string;
  /** The private GitHub repo everything syncs through, as a web address. */
  sync_space: string | null;
  schedule_error: string | null;
  /** Whether the sync indicator is on the Omarchy bar. */
  bar_indicator: boolean;
};

export type Report = { notes: string[]; waiting: string[] };
/** A clashing file as it is here (`mine`) and on the other computer (`theirs`); null when that
 * side removed it. `source` is its path in the sync folder, which choices are keyed by. */
export type Versions = {
  path: string;
  source: string;
  mine: string | null;
  theirs: string | null;
  binary: boolean;
};
export type Conflict = {
  files: string[];
  by: [string, number] | null;
  versions: Versions[];
  /** Files that combined without a clash but came out broken, which is why they need a choice. */
  broken: string[];
};
export type Keep = 'Mine' | 'Theirs';
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
  /** `justSynced`: right after a sync, which already saved and fetched. */
  overview: (onStep: OnStep, justSynced = false) =>
    invoke<Status>('overview', { onStep: steps(onStep), justSynced }),
  /** Settles a clash with the version chosen for each clashing file, then syncs. */
  resolveConflict: (choices: Record<string, Keep>, onStep: OnStep) =>
    invoke<Report>('resolve_conflict', { choices, onStep: steps(onStep) }),
  syncNow: (prefer: Prefer, onStep: OnStep) =>
    invoke<Report>('sync_now', { prefer, onStep: steps(onStep) }),
  undo: (sha: string, onStep: OnStep) => invoke<Report>('undo', { sha, onStep: steps(onStep) }),
  installHelpers: () => invoke<void>('install_helpers'),
  signIn: () => invoke<void>('sign_in'),
  start: (onStep: OnStep) => invoke<void>('start', { onStep: steps(onStep) }),
  /** Returns where this computer's previous settings were backed up. */
  join: (onStep: OnStep) => invoke<string>('join', { onStep: steps(onStep) }),
  installApps: () => invoke<void>('install_apps'),
  /** Adds an extra repository this computer needs (asks for the password). */
  addRepository: (name: string) => invoke<void>('add_repository', { name }),
  tryOmarchy: () => invoke<Data>('try_omarchy'),
  putBackOmarchy: () => invoke<void>('put_back_omarchy'),
  updateOmarchy: () => invoke<void>('update_omarchy'),
  /** Opens a new GitHub issue: the choice of kind, or a problem report filled in with an error. */
  report: (message?: string, details?: string) =>
    invoke<void>('report', { message: message ?? null, details: details ?? null }),
  /** Puts the sync indicator on the Omarchy bar, or takes it off. */
  setBarIndicator: (on: boolean) => invoke<void>('set_bar_indicator', { on }),
  setAppScript: (app: string, command: string) => invoke<Data>('set_app_script', { app, command }),
  removeAppScript: (app: string) => invoke<Data>('remove_app_script', { app }),
  /** Installs one waiting app with its script, in a terminal window the person can see. */
  installWithScript: (app: string, onStep: OnStep) =>
    invoke<void>('install_with_script', { app, onStep: steps(onStep) }),
  /** Whether the sync choices differ from what was last shared. */
  choicesChanged: () => invoke<boolean>('choices_changed'),
  setPlugin: (computer: string, plugin: string, on: boolean) =>
    invoke<Data>('set_plugin', { computer, plugin, on }),
  /** Adds the synced plugins this computer is waiting for. */
  installPlugins: () => invoke<Data>('install_plugins'),
  setApp: (computer: string, app: string, on: boolean) =>
    invoke<Data>('set_app', { computer, app, on }),
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
