import { useState } from 'react';
import { CircleAlert, CircleCheck } from 'lucide-react';
import type { SyncState } from '../state';
import { Link } from '../ui';

/** The result of the last action, with the raw error hidden until someone helping asks for it. */
export function NoticeLine({ s }: { s: SyncState }) {
  const [open, setOpen] = useState(false);
  const n = s.notice;
  if (!n) return null;
  const Icon = n.good ? CircleCheck : CircleAlert;
  return (
    <output className={`notice ${n.good ? 'good' : 'bad'}`}>
      <div className="line">
        <Icon size={15} aria-hidden="true" />
        <span>{n.text}</span>
      </div>
      {n.details &&
        (open ? (
          <pre className="details">{n.details}</pre>
        ) : (
          <div className="indent">
            <Link onPress={() => setOpen(true)}>Show Details</Link>
          </div>
        ))}
    </output>
  );
}
