import { useState } from 'react';
import { CircleAlert, CircleCheck } from 'lucide-react';
import { api } from '../api';
import type { SyncState } from '../state';
import { Link } from '../ui';

/** The result of the last action, with the raw error hidden until someone helping asks for it. */
export function NoticeLine({ s }: { s: SyncState }) {
  const [open, setOpen] = useState(false);
  const n = s.notice;
  if (!n) return null;
  const Icon = n.good && !n.warn ? CircleCheck : CircleAlert;
  return (
    <output className={`notice ${n.warn ? 'warn' : n.good ? 'good' : 'bad'}`}>
      <div className="line">
        <Icon size={15} aria-hidden="true" />
        <span>{n.text}</span>
      </div>
      {open && n.details && <pre className="details">{n.details}</pre>}
      {(!n.good || (n.details && !open)) && (
        <div className="indent inline">
          {n.details && !open && <Link onPress={() => setOpen(true)}>Show Details</Link>}
          {/* Any error can be reported as it is: the issue opens on GitHub, filled in. */}
          {!n.good && (
            <Link onPress={() => api.report(n.text, n.details).catch(s.fail)}>Report This…</Link>
          )}
        </div>
      )}
    </output>
  );
}
