import { Puzzle } from 'lucide-react';
import { api, type Folder } from '../api';
import type { SyncState } from '../state';
import { Group, Link, Push, Row, Tile, Toggle } from '../ui';

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

export function PluginsPane({ s }: { s: SyncState }) {
  const plugins = s.overview?.plugins ?? [];
  const mine = s.me.plugins ?? [];
  const waiting = s.pluginsWaiting.map((p) => p.id);
  return (
    <>
      <FolderGroup
        s={s}
        pick={(f) => PLUGINS.includes(f.path)}
        footer="Each computer chooses its plugins: one added on another computer waits here until you add it. Their settings, and where they sit on the bar, are kept in the bar layout."
      />
      <Group title="Your Plugins">
        {plugins.length === 0 && <Row title="No plugins on any computer yet." />}
        {plugins.map((p) => {
          const others = s.whoHas(p.id);
          const where = mine.includes(p.id)
            ? others.length > 0
              ? `Here and on ${others.join(' and ')}`
              : 'Only on this computer'
            : `On ${others.join(' and ')}`;
          return (
            <Row
              key={p.id}
              lead={<Tile icon={Puzzle} kind="plain" size={26} />}
              title={p.name}
              detail={
                p.description ? `${p.description.replace(/\.$/, '')}. ${where}.` : `${where}.`
              }
              end={
                waiting.includes(p.id) && (
                  <Push disabled={!s.idle} onPress={() => s.edit(() => api.installPlugins(p.id))}>
                    Add Here
                  </Push>
                )
              }
            />
          );
        })}
      </Group>
    </>
  );
}
