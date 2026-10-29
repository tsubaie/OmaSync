// Typed calls into the Rust side (src-tauri/src/main.rs). The types mirror the Rust structs;
// keep them in step when a command changes.

import { Channel, invoke } from '@tauri-apps/api/core';

export type Look = { colors_toml: string; motion: boolean };
export type Folder = {
  path: string;
  name: string;
  default: boolean;
  exists: boolean;
  /** The app whose settings it holds (by package name), for app-data folders. */
  app: string | null;
};
export type Catalog = { folders: Folder[]; data_label: string };
/** A plugin as Omarchy sees it here: `enabled` is null when this computer doesn't have it. */
export type Plugin = {
  id: string;
  name: string;
  description: string;
  /** What it adds: bar-widget, panel, overlay, service, bar. */
  kinds: string[];
  /** Part of Omarchy itself: only ever on or off. */
  first_party: boolean;
  here: boolean;
  enabled: boolean | null;
  /** The active bar can't be switched off. */
  can_disable: boolean;
  /** Its git address, when it came from git (so it can be updated); for one of Omarchy's own,
   * its folder in Omarchy's repository. */
  source: string | null;
  /** From git, as of the day's check: `newer` when its source has a commit this copy lacks,
   * `current` when it doesn't (even if this copy is ahead), `edited` when it has uncommitted
   * changes here (Omarchy won't update over those). */
  update: 'current' | 'newer' | 'edited' | null;
};
/** What updating plugins came to, in sentences; `warn` when one was skipped or failed. */
export type PluginsUpdated = { text: string; warn: boolean };
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
  /** Every folder setup offers; `default`: a first computer syncs it unless told otherwise. */
  folders: { path: string; default: boolean }[];
  /** A first computer's own apps and plugins, or those the synced settings share when `found`. */
  apps: string[];
  plugins: { id: string; name: string }[];
};
/** A safety backup in ~/.local/share/omarchy-sync/backups. */
export type BackupEntry = {
  /** Its file name, which restoreBackup takes. */
  name: string;
  /** What it was made before, in words. */
  label: string;
  /** Unix time. */
  at: number;
  /** The folders and files in it, under home. */
  folders: string[];
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
  /** The newer Omarchy release (major * 1000 + minor) whose settings it's trying anyway. */
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
  /** Paths under home changed here and not synced yet. */
  unshared: string[];
  /** What changed in the choices (the data label's entry), in sentences. */
  choice_changes: string[];
  incoming: number;
  online: boolean;
  /** [computer name, last sync as unix time], this computer first. */
  computers: [string, number | null][];
  apps: {
    installed: string[];
    mine: string[];
    /** Those of `mine` from neither Arch's repos nor the AUR. */
    outside: string[];
    /** Where each app comes from: omarchy, arch, aur, distro (Debian, Fedora), elsewhere, or another repository's name. */
    source: Record<string, string>;
    about: Record<string, string>;
    /** Each app's own website, as its package names it. */
    website: Record<string, string>;
  };
  incoming_files: string[];
  incoming_from: string[];
  history: Entry[];
  /** Every computer's plugins. */
  plugins: Plugin[];
  /** Extra repositories this computer needs for apps it should have. */
  missing_repos: MissingRepo[];
  repos_here: MissingRepo[];
  /** Whether a snapshot of the system is taken before installing apps or adding a repository. */
  snapper_available: boolean;
  /** The safety backups, newest first. */
  backups: BackupEntry[];
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
  /** This copy's version, and how it was installed. */
  version: string;
  installed_how: How;
};

/** How the snapshot of this computer's system before a change went: its number, no Snapper
 * here, failed (the change still went ahead), or the password window was closed. */
export type Snapshot = { Taken: number } | 'Unavailable' | { Failed: string } | 'Cancelled';
export type Report = {
  notes: string[];
  waiting: string[];
  /** Taken before installing apps without asking; null when nothing was installed. */
  snapshot: Snapshot | null;
};
/** How this copy was installed: the release package, the AUR package, or built from source. */
export type How = 'Package' | 'Aur' | 'Source';
/** A newer release of Omarchy Sync. */
export type Update = { version: string };
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
/** Which pieces of the macOS theme are installed, as its `install.sh --status --json` says. */
export type MacosTheme = {
  active: boolean;
  features: Record<string, boolean>;
  /** Missing from a copy of the theme older than its settings. */
  settings?: Record<string, number>;
};
export type Keep = 'Mine' | 'Theirs';
/** What a failed command throws. */
export type Problem =
  { kind: 'Failed'; message: string; details: string } | ({ kind: 'Conflict' } & Conflict);

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
  syncNow: (onStep: OnStep) => invoke<Report>('sync_now', { onStep: steps(onStep) }),
  undo: (sha: string, onStep: OnStep) => invoke<Report>('undo', { sha, onStep: steps(onStep) }),
  installHelpers: () => invoke<void>('install_helpers'),
  signIn: () => invoke<void>('sign_in'),
  /** `folders`, `apps`, `plugins`: the ones chosen to sync; the defaults when left out. */
  start: (onStep: OnStep, folders?: string[], apps?: string[], plugins?: string[]) =>
    invoke<void>('start', {
      folders: folders ?? null,
      apps: apps ?? null,
      plugins: plugins ?? null,
      onStep: steps(onStep),
    }),
  /** Returns where this computer's previous settings were backed up. `skipFolders`: the ones
   * this computer keeps its own version of; `skipApps`, `skipPlugins`: the synced ones it leaves out. */
  join: (onStep: OnStep, skipFolders?: string[], skipApps?: string[], skipPlugins?: string[]) =>
    invoke<string>('join', {
      skipFolders: skipFolders ?? null,
      skipApps: skipApps ?? null,
      skipPlugins: skipPlugins ?? null,
      onStep: steps(onStep),
    }),
  /** Puts a safety backup's files back, after backing up what they replace. */
  restoreBackup: (name: string) => invoke<void>('restore_backup', { name }),
  /** Installs the waiting apps, or only those in `only`. */
  installApps: (only?: string[]) => invoke<Snapshot>('install_apps', { only: only ?? null }),
  /** Adds an extra repository this computer needs (asks for the password). */
  addRepository: (name: string) => invoke<Snapshot>('add_repository', { name }),
  tryOmarchy: () => invoke<Data>('try_omarchy'),
  putBackOmarchy: () => invoke<void>('put_back_omarchy'),
  updateOmarchy: () => invoke<void>('update_omarchy'),
  /** Opens a new GitHub issue: the choice of kind, or a problem report filled in with an error. */
  report: (message?: string, details?: string) =>
    invoke<void>('report', { message: message ?? null, details: details ?? null }),
  /** Puts the sync indicator on the Omarchy bar, or takes it off. */
  setBarIndicator: (on: boolean) => invoke<void>('set_bar_indicator', { on }),
  /** Null until the macOS theme has been downloaded. */
  macosTheme: () => invoke<MacosTheme | null>('macos_theme_status'),
  /** Installs and removes pieces of the macOS theme; returns what its script printed. */
  applyMacosTheme: (
    add: string[],
    remove: string[],
    set: [string, number][],
    switchTo: string | null,
    onStep: OnStep,
  ) => invoke<string>('macos_theme_apply', { add, remove, set, switchTo, onStep: steps(onStep) }),
  /** The themes to switch to before the macOS theme is removed. */
  otherThemes: () => invoke<string[]>('macos_theme_others'),
  /** Gets the latest macOS theme and installs the installed pieces again. */
  updateMacosTheme: (onStep: OnStep) =>
    invoke<string>('macos_theme_update', { onStep: steps(onStep) }),
  setAppScript: (app: string, command: string) => invoke<Data>('set_app_script', { app, command }),
  removeAppScript: (app: string) => invoke<Data>('remove_app_script', { app }),
  /** Installs one waiting app with its script, in a terminal window the person can see. */
  installWithScript: (app: string, onStep: OnStep) =>
    invoke<Snapshot>('install_with_script', { app, onStep: steps(onStep) }),
  /** Whether the sync choices differ from what was last shared. */
  /** What changed in the choices since they were last shared, in sentences; none when nothing. */
  choicesChanged: () => invoke<string[]>('choices_changed'),
  /** A plugin's one switch: synced on every computer, or on none. */
  setPlugin: (plugin: string, on: boolean) => invoke<Data>('set_plugin', { plugin, on }),
  /** Leaves a synced plugin out on this computer only, or takes it back. */
  pluginHere: (plugin: string, on: boolean) => invoke<Data>('plugin_here', { plugin, on }),
  /** Adds the synced plugins this computer is waiting for, or only those in `only`. */
  installPlugins: (only?: string[]) => invoke<Data>('install_plugins', { only: only ?? null }),
  /** Switches a plugin on or off in the Omarchy shell on this computer. */
  setPluginEnabled: (id: string, on: boolean) => invoke<void>('set_plugin_enabled', { id, on }),
  /** Adds a plugin from its git address and switches it on here. */
  addPlugin: (url: string) => invoke<void>('add_plugin', { url }),
  /** Updates one plugin from git, or all of them (a step each); returns a summary. */
  updatePlugins: (id: string | undefined, onStep: OnStep) =>
    invoke<PluginsUpdated>('update_plugins', { id: id ?? null, onStep: steps(onStep) }),
  /** Removes a plugin here now, and from the computers that sync it at their next sync. */
  removePlugin: (id: string) => invoke<void>('remove_plugin', { id }),
  /** An app's one switch: synced on every computer, or on none. */
  setApp: (app: string, on: boolean) => invoke<Data>('set_app', { app, on }),
  /** Leaves a synced app out on this computer only, or takes it back. */
  appHere: (app: string, on: boolean) => invoke<Data>('app_here', { app, on }),
  setFolder: (path: string, on: boolean) => invoke<Data>('set_folder', { path, on }),
  folderHere: (path: string, on: boolean) => invoke<Data>('folder_here', { path, on }),
  setAuto: (on: boolean) => invoke<Data>('set_auto', { on }),
  setInstallAuto: (on: boolean) => invoke<Data>('set_install_auto', { on }),
  renameComputer: (name: string) => invoke<Data>('rename_computer', { name }),
  forgetComputer: (id: string) => invoke<Data>('forget_computer', { id }),
  openSyncSpace: () => invoke<void>('open_sync_space'),
  /** Returns where the local sync folder was moved. */
  disconnect: () => invoke<string>('disconnect'),
  /** A newer release, or null when this copy is the latest. Nothing is downloaded. */
  checkUpdate: () => invoke<Update | null>('check_update'),
  /** Installs that release (it must still be the latest), then restarts the app. */
  installUpdate: (version: string, onStep: OnStep) =>
    invoke<void>('install_update', { version, onStep: steps(onStep) }),
  /** Opens a release's page on GitHub. */
  openRelease: (version: string) => invoke<void>('open_release', { version }),
  /** Opens an app's package page (Arch or the AUR), its own website, or a plugin's source. */
  openPage: (kind: 'arch' | 'aur' | 'website' | 'plugin', name: string) =>
    invoke<void>('open_page', { kind, name }),
};
