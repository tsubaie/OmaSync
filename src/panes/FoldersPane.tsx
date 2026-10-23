import { api } from '../api';
import type { SyncState } from '../state';
import { Group, Link, Row, Toggle } from '../ui';

export function FoldersPane({ s }: { s: SyncState }) {
  const data = s.data;
  if (!data) return null;
  const separate = data.computers?.[s.host]?.skip_files ?? [];
  const folders = s.catalog.folders.filter((f) => data.folders.includes(f.path) || f.exists);
  return (
    <Group
      title="Same on Every Computer"
      footer="Your display layout (monitors.lua) always stays separate on each computer, because every screen is different."
    >
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
