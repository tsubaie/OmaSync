import { Monitor, Undo2, Upload } from 'lucide-react';
import { ago } from '../format';
import type { SyncState } from '../state';
import { Group, Link, Push, Row, Tile } from '../ui';

export function HistoryPane({ s }: { s: SyncState }) {
  const history = s.overview?.history;
  if (!history) return null;
  return (
    <Group footer="Every sync is kept. Undo reverses one change on all your computers; the change itself stays in this list.">
      {history.length === 0 && <Row title="Nothing has been synced yet." />}
      {history.map((e) => {
        const what = e.first
          ? `${s.nameOf(e.computer)} started syncing`
          : e.is_undo
            ? `${s.nameOf(e.computer)} undid an earlier change`
            : e.labels.length === 0
              ? `${s.nameOf(e.computer)} synced`
              : `${s.nameOf(e.computer)} changed ${e.labels.join(', ').toLowerCase()}`;
        const [icon, kind] = e.is_undo
          ? [Undo2, 'history']
          : e.computer === s.host
            ? [Upload, 'sync']
            : [Monitor, 'plain'];
        const end =
          e.first || e.is_undo ? null : s.confirmUndo === e.sha ? (
            <div className="actions">
              <Link onPress={() => s.setConfirmUndo(null)}>Cancel</Link>
              <Push
                disabled={!s.idle}
                onPress={() => {
                  s.setConfirmUndo(null);
                  s.sync('Ask', e.sha);
                }}
              >
                Undo on All Computers
              </Push>
            </div>
          ) : (
            <Link disabled={!s.idle} onPress={() => s.setConfirmUndo(e.sha)}>
              Undo
            </Link>
          );
        return (
          <Row
            key={e.sha}
            lead={<Tile icon={icon} kind={kind} size={24} />}
            title={what}
            detail={ago(e.when, s.now)}
            end={end}
          />
        );
      })}
    </Group>
  );
}
