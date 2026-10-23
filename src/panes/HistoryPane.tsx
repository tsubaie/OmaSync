import { useState } from 'react';
import { Monitor, Undo2, Upload } from 'lucide-react';
import type { Entry } from '../api';
import { ago, byFolder, plural } from '../format';
import type { SyncState } from '../state';
import { Group, Link, Push, Row, Tile } from '../ui';

/** What one sync changed, grouped by the names people know ("Ghostty terminal: Changed config"). */
function Changes({ e, s }: { e: Entry; s: SyncState }) {
  const how = new Map(e.files.map((f) => [f.path.split('/').pop()!, f.how]));
  return (
    <ul className="changes">
      {byFolder(
        e.files.map((f) => f.path),
        s.catalog,
      ).map(([label, names]) => (
        <li key={label}>
          <span className="label">{label}</span>
          {names.length > 0 && (
            <span className="detail">
              {' '}
              {names.map((n) => `${how.get(n) ?? 'Changed'} ${n}`).join(', ')}
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}

export function HistoryPane({ s }: { s: SyncState }) {
  const [open, setOpen] = useState<string | null>(null);
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
            : e.is_merge
              ? `${s.nameOf(e.computer)} settled changes made on two computers`
              : e.labels.length === 0
                ? `${s.nameOf(e.computer)} synced`
                : `${s.nameOf(e.computer)} changed ${e.labels.join(', ').toLowerCase()}`;
        // Every entry in Omarchy's colour; the icon tells undo, this computer and others apart.
        const [icon, kind] = e.is_undo
          ? [Undo2, 'accent']
          : e.computer === s.host
            ? [Upload, 'accent']
            : [Monitor, 'accent'];
        const end =
          e.first || e.is_undo || e.is_merge ? null : s.confirmUndo === e.sha ? (
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
        const confirming = s.confirmUndo === e.sha;
        const shown = confirming || open === e.sha;
        return (
          <Row
            key={e.sha}
            lead={<Tile icon={icon} kind={kind} size={24} />}
            title={what}
            detail={ago(e.when, s.now)}
            end={end}
          >
            {e.files.length > 0 && !confirming && (
              <div className="inline">
                <Link onPress={() => setOpen(shown ? null : e.sha)}>
                  {shown ? 'Hide Files' : `Show ${plural(e.files.length, 'File')}`}
                </Link>
              </div>
            )}
            {confirming && (
              <p className="detail">
                Undo puts these back as they were before this sync, on every computer:
              </p>
            )}
            {shown && <Changes e={e} s={s} />}
          </Row>
        );
      })}
    </Group>
  );
}
