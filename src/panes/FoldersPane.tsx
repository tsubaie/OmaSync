import { useState } from 'react';
import { Input, Label, Text, TextField } from 'react-aria-components';
import { api, type Folder, type Plugin } from '../api';
import type { SyncState } from '../state';
import { Caution, ComputerList, Group, Link, Push, Row, Toggle } from '../ui';

/** Omarchy's own parts, its window settings and its plugins, each listed in the order shown
 * ("other" last); every other folder is an app's data. */
const OMARCHY = [
  '.config/omarchy/themes',
  '.config/omarchy/extensions',
  '.config/omarchy/hooks',
  '.config/omarchy',
];
const WINDOWS = [
  '.config/hypr/bindings.lua',
  '.config/hypr/input.lua',
  '.config/hypr/looknfeel.lua',
  '.config/hypr/autostart.lua',
  '.config/hypr/hyprsunset.conf',
  '.config/hypr',
];
const PLUGINS = ['.config/omarchy/plugins', '.config/omarchy/shell.json'];
const isOmarchy = (f: Folder) => [...OMARCHY, ...WINDOWS, ...PLUGINS].includes(f.path);

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
            detail={`~/${f.path}`}
            end={
              <Toggle on={synced} label={`Sync ${f.name}`} disabled={!s.idle} onChange={toggle} />
            }
          >
            {synced && (
              <div className="inline">
                {!here && (
                  <span className="detail warn">
                    Not synced on this computer: it keeps its own version while your other computers
                    stay in step.
                  </span>
                )}
                <Link
                  disabled={!s.idle}
                  onPress={() => s.edit(() => api.folderHere(f.path, !here))}
                >
                  {here ? "Don't Sync on This Computer" : 'Sync on This Computer Again'}
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

export function AppDataPane({ s }: { s: SyncState }) {
  return <FolderGroup s={s} pick={(f) => !isOmarchy(f)} />;
}

/** Like Apps: one list in a fixed order, one switch per computer. */
export function PluginsPane({ s }: { s: SyncState }) {
  const { data } = s;
  const plugins = s.overview?.plugins ?? [];
  const pluginsOn = !!data?.folders.includes('.config/omarchy/plugins');
  const mine = plugins.filter((p) => !p.first_party);
  const omarchys = plugins.filter((p) => p.first_party);
  const fromGit = mine.filter((p) => p.here && p.source);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<string | null>(null);
  const { computers, name, column, columns, many } = s.matrix;
  /** Runs one of Omarchy's plugin commands, then looks again. */
  const omarchy = (label: string, work: () => Promise<unknown>, said?: (out: string) => void) =>
    s.act(label, work, (out) => {
      said?.(String(out ?? ''));
      return s.refresh();
    });
  const updated = (out: string) =>
    s.setNotice({ good: true, text: out.trim() || 'Your plugins are up to date.' });
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
            footer="Sync puts a plugin on a computer after you approve it there; switching it off never removes it. Enabled is part of your bar layout, so it reaches every computer. Update and Remove only change this computer."
          >
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
            {mine.length > 0 && computers.length > 1 && !many && (
              <div className="row matrix head" style={columns} aria-hidden="true">
                <span>Plugin</span>
                {computers.map((c) => (
                  <span key={c}>{column(c)}</span>
                ))}
              </div>
            )}
            {mine.map((p) => (
              <div className={`row matrix ${many ? 'many' : ''}`} style={columns} key={p.id}>
                <PluginText
                  s={s}
                  p={p}
                  removing={removing === p.id}
                  onRemoving={(on) => setRemoving(on ? p.id : null)}
                  onUpdate={() =>
                    omarchy(`Updating ${p.name}`, () => api.updatePlugins(p.id), updated)
                  }
                  onRemove={() => omarchy(`Removing ${p.name}`, () => api.removePlugin(p.id))}
                />
                {many ? (
                  <ComputerList
                    subject={p.name}
                    computers={computers.map((c) => ({
                      id: c,
                      name: name(c),
                      on: s.wantsPlugin(c, p.id),
                    }))}
                    disabled={!s.idle}
                    onChange={(c, on) => s.edit(() => api.setPlugin(c, p.id, on))}
                  />
                ) : (
                  computers.map((c) => (
                    <div className="cell" key={c}>
                      <span className="cell-label">{column(c)}</span>
                      <Toggle
                        on={s.wantsPlugin(c, p.id)}
                        label={`${p.name} synced on ${name(c)}`}
                        disabled={!s.idle}
                        onChange={(on) => s.edit(() => api.setPlugin(c, p.id, on))}
                      />
                    </div>
                  ))
                )}
              </div>
            ))}
          </Group>
          {fromGit.length > 1 && (
            <div className="actions">
              <Push
                disabled={!s.idle}
                onPress={() => omarchy('Updating your plugins', () => api.updatePlugins(), updated)}
              >
                Update All {fromGit.length} Plugins
              </Push>
            </div>
          )}
          {omarchys.length > 0 && (
            <Group
              title="Omarchy's Own Plugins"
              footer="These come with Omarchy, so there's nothing to sync: enable or disable them here, and the bar layout carries the choice to your other computers."
            >
              {omarchys.map((p) => (
                <div className="row" key={p.id}>
                  <PluginText s={s} p={p} />
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

/**
 * A plugin's name, kind and description, and under them, for one that's on this computer, its
 * Enabled switch (the shell's on or off, kept in the bar layout) and Update and Remove.
 */
function PluginText({
  s,
  p,
  removing = false,
  onRemoving,
  onUpdate,
  onRemove,
}: {
  s: SyncState;
  p: Plugin;
  removing?: boolean;
  onRemoving?: (on: boolean) => void;
  onUpdate?: () => void;
  onRemove?: () => void;
}) {
  const { data, host } = s;
  const synced = !!data?.plugins?.includes(p.id);
  const missingHere = synced && s.wantsPlugin(host, p.id) && !p.here;
  /** Where an unshared plugin is, for the computers that have it. */
  const only = () => {
    const where = [...(p.here ? ['this computer'] : []), ...s.whoHas(p.id)];
    return where.length ? `Only on ${where.join(' and ')}` : '';
  };
  const detail = missingHere
    ? 'Added here after you approve it'
    : p.description || (!p.first_party && !synced ? only() : '');
  const kinds = p.kinds.map((k) => KINDS[k] ?? k).join(', ');
  return (
    <div className="text">
      <div>
        {p.name}
        {kinds && <span className="source">{kinds}</span>}
      </div>
      {detail && <div className={`detail ${missingHere ? 'warn' : ''}`}>{detail}</div>}
      {p.source && (
        <div className="inline">
          <Link out onPress={() => api.openPage('plugin', p.source!).catch(s.fail)}>
            From {place(p.source)}
          </Link>
        </div>
      )}
      {p.here && !removing && (
        <div className="plugin-here">
          <span className="who">
            Enabled
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
          </span>
          {onUpdate && p.source && (
            <Link disabled={!s.idle} onPress={onUpdate}>
              Update Plugin
            </Link>
          )}
          {onRemoving && (
            <Link disabled={!s.idle} onPress={() => onRemoving(true)}>
              Remove Plugin…
            </Link>
          )}
        </div>
      )}
      {removing && (
        <div className="confirm">
          <p className="detail">
            Removes {p.name} from this computer only. Your other computers keep it, and the bar
            layout keeps its settings.
          </p>
          <div className="actions">
            <Link onPress={() => onRemoving?.(false)}>Cancel</Link>
            <Push
              disabled={!s.idle}
              onPress={() => {
                onRemoving?.(false);
                onRemove?.();
              }}
            >
              Remove from This Computer
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
