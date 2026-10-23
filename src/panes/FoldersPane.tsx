import { api, type Folder } from '../api';
import type { SyncState } from '../state';
import { Group, Link, Row, Toggle } from '../ui';

/** Folders that make up Omarchy itself, and its plugins; every other folder is an app's data. */
const OMARCHY = ['.config/omarchy', '.config/hypr'];
const PLUGINS = ['.config/omarchy/plugins', '.config/omarchy/shell.json'];
const isOmarchy = (f: Folder) => OMARCHY.includes(f.path) || PLUGINS.includes(f.path);

function FolderGroup({
  s,
  pick,
  footer,
}: {
  s: SyncState;
  pick: (f: Folder) => boolean;
  footer?: string;
}) {
  const data = s.data;
  if (!data) return null;
  const separate = data.computers?.[s.host]?.skip_files ?? [];
  const folders = s.catalog.folders.filter(
    (f) => pick(f) && (data.folders.includes(f.path) || f.exists),
  );
  return (
    <Group title="Same on Every Computer" footer={footer}>
      {folders.length === 0 && <Row title="None of these apps are on this computer." />}
      {folders.map((f) => {
        const synced = data.folders.includes(f.path);
        const here = !separate.includes(f.path);
        const toggle = (on: boolean) =>
          s.act(on ? 'Adding' : 'Removing', () => api.setFolder(f.path, on), s.refresh);
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
                {!here && <span className="detail warn">This computer keeps its own version.</span>}
                <Link
                  disabled={!s.idle}
                  onPress={() => s.edit(() => api.folderHere(f.path, !here))}
                >
                  {here ? 'Keep a Separate Version Here' : 'Use the Synced Version'}
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
    <FolderGroup
      s={s}
      pick={(f) => OMARCHY.includes(f.path)}
      footer="Your display layout (monitors.lua) always stays separate on each computer, because every screen is different."
    />
  );
}

export function AppDataPane({ s }: { s: SyncState }) {
  return <FolderGroup s={s} pick={(f) => !isOmarchy(f)} />;
}

/** Like Apps: one list in a fixed order, one switch per computer. */
export function PluginsPane({ s }: { s: SyncState }) {
  const { data, host } = s;
  const plugins = s.overview?.plugins ?? [];
  const mine = s.me.plugins ?? [];
  const pluginsOn = !!data?.folders.includes('.config/omarchy/plugins');
  const computers = [host, ...Object.keys(data?.computers ?? {}).filter((c) => c !== host)];
  const name = (c: string) => (c === host ? 'This Computer' : s.nameOf(c));
  // This computer's column is simply "Sync"; the others are named after their computer.
  const column = (c: string) => (c === host ? 'Sync' : s.nameOf(c));
  const columns = { '--n': computers.length } as React.CSSProperties;
  const onlyOn = (id: string) => {
    const where = [...(mine.includes(id) ? ['this computer'] : []), ...s.whoHas(id)];
    return where.length ? `Only on ${where.join(' and ')}` : '';
  };
  return (
    <>
      <FolderGroup
        s={s}
        pick={(f) => PLUGINS.includes(f.path)}
        footer="Plugins are the add-ons themselves; their settings, and where they sit on the bar, are kept in the bar layout."
      />
      {pluginsOn && (
        <Group
          title="Your Plugins"
          footer="Switch a plugin on for a computer to have it there; it's added after you approve it. Switching it off never removes it."
        >
          {plugins.length === 0 ? (
            <Row title="No plugins on any computer yet." />
          ) : (
            <>
              <div className="row matrix head" style={columns} aria-hidden="true">
                <span>Plugin</span>
                {computers.map((c) => (
                  <span key={c}>{column(c)}</span>
                ))}
              </div>
              {plugins.map((p) => {
                const synced = !!data?.plugins?.includes(p.id);
                const missingHere = synced && s.wantsPlugin(host, p.id) && !mine.includes(p.id);
                return (
                  <div className="row matrix" style={columns} key={p.id}>
                    <div className="text">
                      <div>{p.name}</div>
                      {missingHere ? (
                        <div className="detail warn">Added here after you approve it</div>
                      ) : (
                        <div className="detail">{synced ? p.description : onlyOn(p.id)}</div>
                      )}
                    </div>
                    {computers.map((c) => (
                      <div className="cell" key={c}>
                        <span className="cell-label">{column(c)}</span>
                        <Toggle
                          on={s.wantsPlugin(c, p.id)}
                          label={`${p.name} synced on ${name(c)}`}
                          disabled={!s.idle}
                          onChange={(on) => s.edit(() => api.setPlugin(c, p.id, on))}
                        />
                      </div>
                    ))}
                  </div>
                );
              })}
            </>
          )}
        </Group>
      )}
    </>
  );
}
