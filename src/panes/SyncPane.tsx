import { Button } from 'react-aria-components';
import {
  Check,
  Cloud,
  Download,
  Monitor,
  Package,
  Plus,
  RefreshCw,
  TriangleAlert,
  Upload,
  WifiOff,
  type LucideIcon,
} from 'lucide-react';
import { api } from '../api';
import { ago, byFolder, plural, shortList } from '../format';
import type { SyncState } from '../state';
import { Group, Push, Row, Tile } from '../ui';
import { NoticeLine } from './NoticeLine';

/** One fixed set of states, each with a headline, a detail line, an icon and its colour. */
function status(s: SyncState): [string, string, LucideIcon, string] {
  const { busy, conflict, overview: ov, host } = s;
  if (busy) return [`${busy}…`, 'You can keep working while this runs.', RefreshCw, 'sync'];
  if (conflict) {
    return [
      'The same setting was changed on two computers.',
      'Choose which version to keep, then syncing carries on.',
      TriangleAlert,
      'warn',
    ];
  }
  if (!ov) return ['Checking for changes…', '', RefreshCw, 'sync'];
  const mine = ov.computers.find(([name]) => name === host)?.[1];
  const last = mine ? `Last synced ${ago(mine, s.now)}.` : "This computer hasn't synced yet.";
  if (!ov.online) {
    return [
      "Can't reach your other computers right now.",
      `You seem to be offline. Your changes are kept here until you reconnect. ${last}`,
      WifiOff,
      'bad',
    ];
  }
  const n = ov.unshared.length;
  const changed = plural(n, 'change');
  const updates = plural(ov.incoming, 'update');
  if (n === 0 && ov.incoming === 0)
    return ['Everything matches your other computers.', last, Check, 'good'];
  if (ov.incoming === 0) {
    return [
      `${changed} on this computer ${n === 1 ? "isn't" : "aren't"} shared yet.`,
      last,
      Upload,
      'warn',
    ];
  }
  if (n === 0)
    return [`Your other computers have ${updates} for this one.`, last, Download, 'sync'];
  return [
    `${changed} here and ${updates} from your other computers are waiting.`,
    last,
    Download,
    'sync',
  ];
}

/** One computer in the picture: a rounded screen, its name, and when it last shared changes. */
function Device({
  name,
  when,
  here,
  more,
  badge: Badge,
  s,
}: {
  name: string;
  when: number | null;
  here?: boolean;
  more?: number;
  badge?: LucideIcon;
  s: SyncState;
}) {
  const seen = when ? ago(when, s.now) : 'Not synced yet';
  const label = more ? `${name} +${more}` : name;
  return (
    // Its name and time are visible text, so screen readers need no extra label.
    <div className={`device ${here ? 'here' : ''}`}>
      <span className={`screen ${more ? 'stack' : ''}`}>
        <Monitor size={30} strokeWidth={1.75} />
        {Badge && (
          <span className="badge-dot">
            <Badge size={13} strokeWidth={3} />
          </span>
        )}
      </span>
      <span className="name">{here ? 'This Computer' : label}</span>
      <span className="when">{seen}</span>
    </div>
  );
}

/**
 * The one signature element: this computer, your private sync space, and your other computers,
 * joined by lines that carry light while syncing and rest in the state's colour.
 */
function Hero({ s }: { s: SyncState }) {
  const [head, detail, icon, kind] = status(s);
  const computers = s.overview?.computers ?? [];
  const mine = computers.find(([id]) => id === s.host)?.[1] ?? null;
  const others = computers.filter(([id]) => id !== s.host);
  // The most recently active other computer represents the rest.
  const [lead] = [...others].toSorted((a, b) => (b[1] ?? 0) - (a[1] ?? 0));
  const idle = s.overview && !s.busy && kind !== 'good' ? 'idle' : '';
  return (
    <section className={`box hero ${kind}`} aria-label="Sync status">
      <div className="constellation">
        <Device name={s.nameOf(s.host)} when={mine} here badge={icon} s={s} />
        <span className={`wire ${idle}`} aria-hidden="true" />
        <span className="hub">
          <Cloud size={22} strokeWidth={2} aria-hidden="true" />
          <span className="visually-hidden">Your private sync space</span>
        </span>
        {lead ? (
          <>
            <span className={`wire ${idle}`} aria-hidden="true" />
            <Device name={s.nameOf(lead[0])} when={lead[1]} more={others.length - 1} s={s} />
          </>
        ) : (
          <>
            <span className="wire dashed" aria-hidden="true" />
            <Button className="device empty" onPress={() => s.setTab('settings')}>
              <span className="screen">
                <Plus size={26} strokeWidth={2} />
              </span>
              <span className="name">Add a Computer</span>
              <span className="when">Three short steps</span>
            </Button>
          </>
        )}
      </div>
      <output>
        <h2>{head}</h2>
        {detail && <p className="detail">{detail}</p>}
      </output>
      {!s.conflict && (
        <Push prominent large disabled={!s.idle} onPress={() => s.sync()} tip="Sync Now  Ctrl+R">
          {s.syncing ? 'Syncing…' : 'Sync Now'}
        </Push>
      )}
    </section>
  );
}

function ConflictGroup({ s }: { s: SyncState }) {
  const [id, when] = s.conflict!.by ?? [];
  const name = id && s.nameOf(id);
  // Two equal choices: neither is styled as the recommended one.
  return (
    <Group title="Changed on Both Computers">
      {byFolder(s.conflict!.files, s.catalog).map(([label, files]) => (
        <Row key={label} title={label} detail={files.join(', ')} />
      ))}
      <div className="row choices">
        <div className="choice">
          <Push onPress={() => s.sync('ThisComputer')}>Keep This Computer's Version</Push>
          <span className="sub">What you have here now</span>
        </div>
        <div className="choice">
          <Push onPress={() => s.sync('OtherComputer')}>
            {name ? `Keep ${name}'s Version` : "Keep the Other Computer's Version"}
          </Push>
          <span className="sub">
            {when ? `Changed there ${ago(when, s.now)}` : 'Changed on another computer'}
          </span>
        </div>
      </div>
    </Group>
  );
}

export function SyncPane({ s }: { s: SyncState }) {
  const ov = s.overview;
  if (s.conflict) {
    return (
      <>
        <Hero s={s} />
        <NoticeLine s={s} />
        <ConflictGroup s={s} />
      </>
    );
  }
  const showWaiting = s.waiting.length > 0 && s.waiting.join() !== s.dismissed && s.idle;
  const from =
    ov?.incoming_from.length === 1
      ? `From ${s.nameOf(ov.incoming_from[0])}.`
      : 'From your other computers.';
  return (
    <>
      <Hero s={s} />
      <NoticeLine s={s} />
      {ov && (
        <>
          {showWaiting && (
            // Installing changes the system, so it waits for a yes unless this computer opted out of asking.
            <Group title="Apps Waiting to Install" footer="Omarchy will ask for your password.">
              {s.waiting.map((app) => (
                <Row key={app} lead={<Tile icon={Package} kind="apps" size={24} />} title={app} />
              ))}
              <div className="row actions">
                <Push onPress={() => s.setDismissed(s.waiting.join())}>Not Now</Push>
                <Push onPress={() => s.act('Installing apps', () => api.installApps(), s.refresh)}>
                  Install
                </Push>
              </div>
            </Group>
          )}
          {ov.incoming > 0 && (
            // A preview of what the waiting updates will change here, before syncing applies them.
            <Group
              title="Coming to This Computer"
              footer={`${from} Syncing applies these here, and History can undo them.`}
            >
              {byFolder(ov.incoming_files, s.catalog).map(([label, files]) => (
                <Row key={label} title={label} detail={files.join(', ')} />
              ))}
            </Group>
          )}
          <Group title="Not Shared Yet">
            {ov.unshared.length === 0 ? (
              <Row title="Everything on this computer is shared." />
            ) : (
              byFolder(ov.unshared, s.catalog).map(([label, files]) => (
                <Row key={label} title={label} detail={shortList(files)} />
              ))
            )}
          </Group>
        </>
      )}
    </>
  );
}
