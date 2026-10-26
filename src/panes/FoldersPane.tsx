import { useState } from 'react';
import { Input, Label, Text, TextField } from 'react-aria-components';
import { api, type Folder, type Plugin, type PluginsUpdated } from '../api';
import type { SyncState, Tab as TabId } from '../state';
import { CircleCheck, Download } from 'lucide-react';
import { plural } from '../format';
import { Caution, Group, Link, Progress, Push, Row, Toggle } from '../ui';

/** Omarchy's own parts, its window settings and its plugins, each listed in the order shown
 * ("other" last); every other folder is an app's data. */
export const OMARCHY = [
  '.config/omarchy/themes',
  '.config/omarchy/extensions',
  '.config/omarchy/hooks',
  '.config/omarchy',
];
export const WINDOWS = [
  '.config/hypr/bindings.lua',
  '.config/hypr/input.lua',
  '.config/hypr/looknfeel.lua',
  '.config/hypr/autostart.lua',
  '.config/hypr/hyprsunset.conf',
  '.config/hypr',
];
export const PLUGINS = ['.config/omarchy/plugins', '.config/omarchy/shell.json'];
const isOmarchy = (f: Folder) => [...OMARCHY, ...WINDOWS, ...PLUGINS].includes(f.path);

/** Which tab a sentence from `describe_changes` (src-tauri/src/sync/data.rs) belongs on. */
function tabOf(change: string, folders: Folder[]): TabId | undefined {
  const folder = folders.find((f) => change.startsWith(`${f.name} `));
  if (folder) {
    if (PLUGINS.includes(folder.path)) return 'plugins';
    return OMARCHY.includes(folder.path) || WINDOWS.includes(folder.path) ? 'omarchy' : 'data';
  }
  if (change.startsWith('Plugin ')) return 'plugins';
  if (change.startsWith('Theme set to ') || change === 'Background changed') return 'omarchy';
  if (
    change.startsWith('Automatic sync switched ') ||
    change.startsWith('Synced apps install without asking on ') ||
    change.startsWith('Synced apps wait for approval on ') ||
    change.startsWith('Bar indicator switched ') ||
    change.includes(' renamed to ')
  )
    return 'settings';
  if (
    change.endsWith(' added to the synced apps') ||
    change.endsWith(' taken out of the synced apps') ||
    change.startsWith('Install steps for ') ||
    change.includes(' switched off for ') ||
    change.includes(' switched on for ')
  )
    return 'apps';
  return undefined;
}

/** The unsynced changes that belong on `tab`, so each tab's banner only shows its own. */
export function pendingForTab(pending: string[], tab: TabId, folders: Folder[]): string[] {
  return pending.filter((c) => tabOf(c, folders) === tab);
}

function FolderGroup({
  s,
  pick,
  title = 'Same on Every Computer',
  footer,
}: {
  s: SyncState;
  /** The folders to show: a list (shown in its order) or a test. */
  pick: string[] | ((f: Folder) => boolean);
  title?: string;
  footer?: string;
}) {
  const data = s.data;
  if (!data) return null;
  const separate = data.computers?.[s.host]?.skip_files ?? [];
  const installed = s.overview?.apps.installed ?? [];
  // With other computers, each row says which switch reaches them all and which only this one.
  const many = (s.overview?.computers.length ?? 0) > 1;
  const order = (f: Folder) => (Array.isArray(pick) ? pick.indexOf(f.path) : 0);
  const folders = s.catalog.folders
    .filter(
      (f) =>
        (Array.isArray(pick) ? pick.includes(f.path) : pick(f)) &&
        (data.folders.includes(f.path) || f.exists),
    )
    .toSorted((a, b) => order(a) - order(b));
  return (
    <Group
      title={title}
      footer={
        <>
          {footer && <>{footer} </>}A switch syncs an item on all your computers, or stops it
          everywhere. Don't Sync on This Computer leaves only this computer out.
        </>
      }
    >
      {folders.length === 0 && <Row title="None of these apps are on this computer." />}
      {folders.map((f) => {
        const synced = data.folders.includes(f.path);
        const here = !separate.includes(f.path);
        const toggle = (on: boolean) =>
          s.act(
            on ? 'Adding' : 'Removing',
            () => api.setFolder(f.path, on),
            () => s.refresh(),
          );
        return (
          <Row
            key={f.path}
            title={f.name}
            detail={
              f.app && !installed.includes(f.app)
                ? `~/${f.path}, not installed on this computer`
                : `~/${f.path}`
            }
            end={
              <Toggle
                on={synced}
                label={many ? `Sync ${f.name} (all computers)` : `Sync ${f.name}`}
                disabled={!s.idle}
                onChange={toggle}
              />
            }
          >
            {synced && (many || !here) && (
              <div className="inline">
                {here ? (
                  <span className="detail">On for all your computers, this one included.</span>
                ) : (
                  <span className="detail warn">
                    Not synced on this computer: it keeps its own version while your other computers
                    stay in step.
                  </span>
                )}
                <Link
                  disabled={!s.idle}
                  onPress={() => s.edit(() => api.folderHere(f.path, !here))}
                >
                  {here ? "Don't Sync on This Computer" : 'Sync on This Computer'}
                </Link>
              </div>
            )}
          </Row>
        );
      })}
    </Group>
  );
}

export function OmarchyPane({ s }: { s: SyncState }) {
  return (
    <>
      <FolderGroup
        s={s}
        pick={OMARCHY}
        title="Omarchy"
        footer="Only the theme and background you're using sync, and your other computers switch to them."
      />
      <FolderGroup
        s={s}
        pick={WINDOWS}
        title="Windows"
        footer="Your display layout (monitors.lua) always stays separate on each computer, because every screen is different."
      />
    </>
  );
}

/**
 * An app's settings folder is worth showing when the app is installed here or synced to any
 * computer, or when the folder already syncs: Omarchy ships settings for terminals nobody may use.
 */
export function AppDataPane({ s }: { s: SyncState }) {
  const installed = s.overview?.apps.installed ?? [];
  const relevant = (f: Folder) =>
    !f.app ||
    installed.includes(f.app) ||
    !!s.data?.apps.includes(f.app) ||
    !!s.data?.folders.includes(f.path);
  return <FolderGroup s={s} pick={(f) => !isOmarchy(f) && relevant(f)} />;
}

/** Like Apps: plugins here first, then those your other computers sync that this one lacks. */
export function PluginsPane({ s }: { s: SyncState }) {
  const { data } = s;
  const plugins = s.overview?.plugins ?? [];
  const pluginsOn = !!data?.folders.includes('.config/omarchy/plugins');
  const mine = plugins.filter((p) => !p.first_party);
  const omarchys = plugins.filter((p) => p.first_party);
  const fromGit = mine.filter((p) => p.here && p.source);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<string | null>(null);
  const many = (s.overview?.computers.length ?? 0) > 1;
  /** Runs one of Omarchy's plugin commands, then looks again. */
  const omarchy = <T,>(
    label: string,
    work: (onStep: (step: string) => void) => Promise<T>,
    said?: (out: T) => void,
  ) =>
    s.act(label, work, (out) => {
      said?.(out);
      return s.refresh();
    });
  /** The summary of an update: a warning when a plugin was skipped or failed. */
  const updated = ({ text, warn }: PluginsUpdated) => s.setNotice({ good: true, warn, text });
  const newer = fromGit.filter((p) => p.update === 'newer').length;
  /** One of your plugins: its one Sync switch (every computer), and Install when it's waiting here. */
  const row = (p: Plugin) => {
    const toggle = (
      <Toggle
        on={!!data?.plugins?.includes(p.id)}
        label={many ? `Sync ${p.name} (all computers)` : `Sync ${p.name}`}
        disabled={!s.idle}
        onChange={(on) => s.edit(() => api.setPlugin(p.id, on))}
      />
    );
    return (
      <div className="row" key={p.id}>
        <PluginText
          s={s}
          p={p}
          many={many}
          removing={removing === p.id}
          onRemoving={(on) => setRemoving(on ? p.id : null)}
          onUpdate={() =>
            omarchy(`Updating ${p.name}`, (onStep) => api.updatePlugins(p.id, onStep), updated)
          }
          onRemove={() => omarchy(`Removing ${p.name}`, () => api.removePlugin(p.id))}
        />
        {s.pluginsWaiting.some((w) => w.id === p.id) ? (
          <div className="inline">
            <Push
              disabled={!s.idle}
              onPress={() => omarchy(`Installing ${p.name}`, () => api.installPlugins([p.id]))}
            >
              Install
            </Push>
            {toggle}
          </div>
        ) : (
          toggle
        )}
      </div>
    );
  };
  const updatingAll = s.plan.length > 0 && s.plan.every((step) => step.startsWith('Updating '));
  return (
    <>
      <FolderGroup
        s={s}
        pick={PLUGINS}
        footer="Plugins are the add-ons themselves; their settings, and where they sit on the bar, are kept in the bar layout."
      />
      {pluginsOn && (
        <>
          <Group
            title="Your Plugins"
            footer="A switch syncs a plugin to all your computers, where it's added after you approve it, or stops it everywhere; switching it off never removes it. Removing a synced plugin removes it from every computer that syncs it at its next sync; to keep it on one as its own, choose Don't Sync on This Computer there first."
          >
            {fromGit.length > 0 && (
              <Row
                title="Update your plugins"
                detail={`${plural(fromGit.length, 'plugin')} came from git; Omarchy fetches the newest version of each. ${
                  newer
                    ? `${plural(newer, 'update')} available.`
                    : fromGit.some((p) => p.update === 'current')
                      ? 'All up to date.'
                      : ''
                }`}
                end={
                  <Push
                    prominent={newer > 0}
                    disabled={!s.idle}
                    onPress={() =>
                      omarchy(
                        'Updating your plugins',
                        (onStep) => api.updatePlugins(undefined, onStep),
                        updated,
                      )
                    }
                  >
                    {updatingAll ? 'Updating…' : 'Update All'}
                  </Push>
                }
              >
                {updatingAll && <Progress steps={s.steps} plan={s.plan} percent={s.percent} />}
              </Row>
            )}
            <Row
              title="Add a plugin from its git address"
              detail="Omarchy downloads it and enables it here; your other computers get it when you sync it to them."
              end={
                !adding && (
                  <Push disabled={!s.idle} onPress={() => setAdding(true)}>
                    Add Plugin…
                  </Push>
                )
              }
            >
              {adding && (
                <AddPluginForm
                  s={s}
                  onDone={() => setAdding(false)}
                  onAdd={(url) => omarchy('Adding the plugin', () => api.addPlugin(url))}
                />
              )}
            </Row>
            {mine.length === 0 && <Row title="No plugins of your own on any computer yet." />}
            {mine.filter((p) => p.here).map(row)}
          </Group>
          {mine.some((p) => !p.here) && (
            <Group
              title="Not Installed on This Computer"
              footer="Synced from your other computers. Install one here, or leave this computer out."
            >
              {mine.filter((p) => !p.here).map(row)}
            </Group>
          )}
          {omarchys.length > 0 && (
            <Group
              title="Omarchy's Own Plugins"
              footer="These come with Omarchy, so there's nothing to sync: enable or disable them here, and the bar layout carries the choice to your other computers."
            >
              {omarchys.map((p) => (
                <div className="row" key={p.id}>
                  <PluginText s={s} p={p} />
                  <EnabledToggle s={s} p={p} />
                </div>
              ))}
            </Group>
          )}
        </>
      )}
    </>
  );
}

/** What a plugin adds, in plain words. */
const KINDS: Record<string, string> = {
  'bar-widget': 'Bar widget',
  panel: 'Panel',
  overlay: 'Overlay',
  service: 'Service',
  bar: 'Bar',
};

/** A git address without the noise: github.com/acme/weather. */
const place = (url: string) =>
  url
    .replace(/^https:\/\//, '')
    .replace(/^git@([^:]+):/, '$1/')
    .replace(/\.git$/, '');

/** The shell's on or off for a plugin here, kept in the bar layout (so it syncs). */
function EnabledToggle({ s, p }: { s: SyncState; p: Plugin }) {
  return (
    <Toggle
      on={!!p.enabled}
      label={`${p.name} enabled`}
      disabled={!s.idle || !p.can_disable}
      onChange={(on) =>
        s.act(
          on ? `Enabling ${p.name}` : `Disabling ${p.name}`,
          () => api.setPluginEnabled(p.id, on),
          () => s.refresh(),
        )
      }
    />
  );
}

/**
 * A plugin in a few lines: its name, kind and source; its description, cut to one line (the
 * whole on hover); and, for one of your own, the day's word on updates (Up to date, Changed on
 * this computer with no Update, or Update in the accent colour when there's one), Remove, and
 * like Configurations whether this computer is left out. One of Omarchy's own has only its
 * Enabled switch, at the end of its row.
 */
function PluginText({
  s,
  p,
  many,
  removing = false,
  onRemoving,
  onUpdate,
  onRemove,
}: {
  s: SyncState;
  p: Plugin;
  /** Set for one of your own: whether there are other computers to stay in step with. */
  many?: boolean;
  removing?: boolean;
  onRemoving?: (on: boolean) => void;
  onUpdate?: () => void;
  onRemove?: () => void;
}) {
  const { data, host } = s;
  const synced = !!data?.plugins?.includes(p.id);
  const here = s.wantsPlugin(host, p.id);
  /** Where an unshared plugin is, for the computers that have it. */
  const only = () => {
    const where = [...(p.here ? ['this computer'] : []), ...s.whoHas(p.id)];
    return where.length ? `Only on ${where.join(' and ')}` : '';
  };
  const detail = p.description || (!p.first_party && !synced ? only() : '');
  const kinds = p.kinds.map((k) => KINDS[k] ?? k).join(', ');
  return (
    <div className="text">
      <div className="plugin-name">
        {p.name}
        {kinds && <span className="source">{kinds}</span>}
        {p.source && (
          <Link
            out
            label={`Open ${place(p.source).split('/')[0]} (${p.name})`}
            onPress={() => api.openPage('plugin', p.source!).catch(s.fail)}
          >
            {place(p.source).split('/')[0]}
          </Link>
        )}
      </div>
      {detail && (
        <div className="detail clipped" title={detail}>
          {detail}
        </div>
      )}
      {p.here && !removing && (onUpdate || onRemoving) && (
        <div className="plugin-here">
          {p.update === 'current' && (
            <span className="who good">
              <CircleCheck size={13} strokeWidth={2.25} aria-hidden="true" />
              Up to date
            </span>
          )}
          {p.update === 'edited' && <span className="who">Changed on this computer</span>}
          {onUpdate && p.source && (p.update === 'newer' || p.update === null) && (
            <Link
              accent={p.update === 'newer'}
              label={
                p.update === 'newer'
                  ? `Update (an update is available for ${p.name})`
                  : `Update (${p.name})`
              }
              disabled={!s.idle}
              onPress={onUpdate}
            >
              {p.update === 'newer' && <Download size={13} strokeWidth={2.25} aria-hidden="true" />}
              {s.busy === `Updating ${p.name}` ? 'Updating…' : 'Update'}
            </Link>
          )}
          {onRemoving && (
            <Link
              bad
              label={`Remove… (${p.name})`}
              disabled={!s.idle}
              onPress={() => onRemoving(true)}
            >
              Remove…
            </Link>
          )}
        </div>
      )}
      {many !== undefined && synced && (many || !here) && (
        <div className="inline">
          {here ? (
            <span className="detail">On for all your computers, this one included.</span>
          ) : (
            <span className="detail warn">
              Not synced on this computer: it isn't added here while your other computers stay in
              step.
            </span>
          )}
          <Link disabled={!s.idle} onPress={() => s.edit(() => api.pluginHere(p.id, !here))}>
            {here ? "Don't Sync on This Computer" : 'Sync on This Computer'}
          </Link>
        </div>
      )}
      {removing && (
        <div className="confirm">
          <p className="detail">
            {synced
              ? `Removes ${p.name} from every computer at its next sync.`
              : `Removes ${p.name} from this computer. It isn't synced, so your other computers keep theirs.`}
          </p>
          <div className="actions">
            <Link onPress={() => onRemoving?.(false)}>Cancel</Link>
            <Push
              bad
              disabled={!s.idle}
              onPress={() => {
                onRemoving?.(false);
                onRemove?.();
              }}
            >
              {synced ? 'Remove from Every Computer' : 'Remove from This Computer'}
            </Push>
          </div>
        </div>
      )}
    </div>
  );
}

/** The address of a plugin to add, with the warning that it runs with your access. */
function AddPluginForm({
  s,
  onDone,
  onAdd,
}: {
  s: SyncState;
  onDone: () => void;
  onAdd: (url: string) => void;
}) {
  const [url, setUrl] = useState('');
  const address = url.trim();
  const plausible = /^(https:\/\/|git@)\S+$/.test(address);
  return (
    <div className="script-form">
      <Caution>
        <b>Only add plugins from a source you fully trust.</b> A plugin runs with your access: it
        can read, change or delete anything you can, and it stays on this computer until you remove
        it.
      </Caution>
      <TextField className="field labelled" value={url} onChange={setUrl} isRequired>
        <Label>Git address</Label>
        <Input placeholder="https://github.com/someone/omarchy-plugin.git" spellCheck={false} />
        <Text slot="description">
          The address you would give to git clone. Omarchy checks the plugin before it goes on.
        </Text>
      </TextField>
      <div className="actions">
        <Link onPress={onDone}>Cancel</Link>
        <Push
          disabled={!s.idle || !plausible}
          onPress={() => {
            onDone();
            onAdd(address);
          }}
        >
          Add Plugin
        </Push>
      </div>
    </div>
  );
}
