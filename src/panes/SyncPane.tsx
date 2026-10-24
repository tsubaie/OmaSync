import { useState } from 'react';
import { Button, Radio, RadioGroup } from 'react-aria-components';
import {
  Check,
  Cloud,
  Download,
  Package,
  Plus,
  Puzzle,
  RefreshCw,
  TriangleAlert,
  Upload,
  WifiOff,
  type LucideIcon,
} from 'lucide-react';
import { api, type Keep, type Versions } from '../api';
import { ago, byFolder, diffLines, plural, shortList } from '../format';
import type { SyncState } from '../state';
import { Caution, Group, Link, OmarchyLogo, Progress, Push, Row, Tile } from '../ui';
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
  const synced = mine ? `Last synced ${ago(mine, s.now)}.` : "This computer hasn't synced yet.";
  const last = s.checked ? `${synced} Last checked ${ago(s.checked, s.now)}.` : synced;
  if (!ov.online) {
    return [
      "Can't reach your other computers right now.",
      `You seem to be offline. Your changes are kept here until you reconnect. ${last}`,
      WifiOff,
      'bad',
    ];
  }
  // Counted as the list below shows them (one row per kind of setting), so the two always agree.
  const n = byFolder(ov.unshared, s.catalog).length;
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
  // Several other computers show as one stack: how many, and when the latest synced.
  const label = more ? `${more + 1} Other Computers` : name;
  return (
    // Its name and time are visible text, so screen readers need no extra label.
    <div className={`device ${here ? 'here' : ''}`}>
      <span className={`screen ${more ? 'stack' : ''}`}>
        <OmarchyLogo size={32} />
        {more ? (
          <span className="count" aria-hidden="true">
            {more + 1}
          </span>
        ) : null}
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
            <Button
              className="device empty"
              onPress={() =>
                document.getElementById('add-computer')?.scrollIntoView({ behavior: 'smooth' })
              }
            >
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
      <Progress steps={s.steps} plan={s.plan} percent={s.percent} />
      {!s.conflict && (
        <Push prominent large disabled={!s.idle} onPress={() => s.sync()} tip="Sync Now  Ctrl+R">
          {s.syncing ? 'Syncing…' : 'Sync Now'}
        </Push>
      )}
    </section>
  );
}

/** One clashing file: both versions side by side, the lines that differ marked. */
function Compare({ v, other }: { v: Versions; other: string }) {
  if (v.binary)
    return <p className="detail">This file can't be shown, but you can still choose.</p>;
  const rows = diffLines(v.mine ?? '', v.theirs ?? '');
  return (
    <table className="compare" aria-label={`Both versions of ${v.path}`}>
      <thead>
        <tr>
          <th scope="col">This Computer{v.mine === null ? ' (removed it)' : ''}</th>
          <th scope="col">
            {other}
            {v.theirs === null ? ' (removed it)' : ''}
          </th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r, i) => (
          <tr key={i} className={r.changed ? 'changed' : ''}>
            <td className={r.mine === null ? 'gap' : ''}>
              <code>{r.mine ?? ''}</code>
            </td>
            <td className={r.theirs === null ? 'gap' : ''}>
              <code>{r.theirs ?? ''}</code>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/**
 * Settings changed on both computers: each shown both ways, with a choice of which to keep.
 * Nothing is lost either way: the version not kept stays in History.
 */
function ConflictGroup({ s }: { s: SyncState }) {
  const [choices, setChoices] = useState<Record<string, Keep>>({});
  const [id, when] = s.conflict!.by ?? [];
  const other = id ? s.nameOf(id) : 'The other computer';
  const versions = s.conflict!.versions;
  const left = versions.filter((v) => !choices[v.source]).length;
  return (
    <Group
      title="Changed on Both Computers"
      footer={`${other} changed ${when ? ago(when, s.now) : 'these too'}. Highlighted lines are the ones that differ. The version you don't keep stays in History.`}
    >
      {versions.map((v) => (
        <div className="row clash" key={v.source}>
          <div className="text">
            <div>{byFolder([v.path], s.catalog)[0][0]}</div>
            <div className="detail">~/{v.path}</div>
            {s.conflict!.broken.includes(v.path) && (
              <div className="detail warn">
                Each computer changed a different part of it, but together they don't make a working
                file, so choose one.
              </div>
            )}
          </div>
          <Compare v={v} other={other} />
          <RadioGroup
            className="keep"
            aria-label={`Which version of ${v.path} to keep`}
            orientation="horizontal"
            value={choices[v.source] ?? null}
            onChange={(keep) => setChoices((c) => ({ ...c, [v.source]: keep as Keep }))}
          >
            {/* Each choice sits under the version it keeps. */}
            <Radio className="choice-radio" value="Mine">
              Keep This Computer's Version
            </Radio>
            <Radio className="choice-radio" value="Theirs">
              Keep {other}'s Version
            </Radio>
          </RadioGroup>
        </div>
      ))}
      <div className="row actions">
        {/* Leaves everything as it is; the next sync asks again. */}
        <Link onPress={() => s.setConflict(null)}>Cancel</Link>
        <Push
          prominent
          disabled={left > 0 || !s.idle}
          onPress={() => s.sync(undefined, false, choices)}
        >
          {left > 0 ? `Choose ${left} More` : 'Keep Chosen Versions'}
        </Push>
      </div>
    </Group>
  );
}

/** How another computer joins: it signs in to the same account and picks the synced settings. */
function AddComputer({ s }: { s: SyncState }) {
  const space = s.syncSpace;
  return (
    <div id="add-computer">
      <Group
        title="Add a Computer"
        footer="The new computer joins by itself; nothing needs to change here."
      >
        {[
          ['Install Omarchy Sync on the other computer', 'The same way you installed it here.'],
          [
            'Sign in to the same GitHub account',
            space
              ? `The one that owns ${space.replace('https://github.com/', '')}.`
              : 'The one you use here.',
          ],
          [
            'Choose Use My Synced Settings',
            "It backs up that computer's settings first, then brings in yours.",
          ],
        ].map(([title, detail], i) => (
          <Row
            key={title}
            lead={
              <span className="badge now" aria-hidden="true">
                {i + 1}
              </span>
            }
            title={title}
            detail={detail}
          />
        ))}
      </Group>
    </div>
  );
}

/** What the app does, in a sentence, for anyone opening it for the first time. */
function About() {
  return (
    <p className="lede">
      Keeps your Omarchy settings, themes, plugins and apps the same on all your computers. Changes
      travel through a private space in your GitHub account, and you choose what syncs.
    </p>
  );
}

/**
 * Apps from outside the repos, waiting to be installed with their command. Never automatic:
 * Install first shows the whole command with a warning, and only Run It runs it, in a
 * terminal window the person can watch.
 */
function ScriptedApps({ s }: { s: SyncState }) {
  const [confirm, setConfirm] = useState<string | null>(null);
  return (
    <Group
      title="Apps to Install with Their Steps"
      footer="Each runs in a terminal window, so you can see what it does and type your password if it asks."
    >
      {s.scripted.map(({ app, script }) => (
        <Row
          key={app}
          lead={<Tile icon={Package} kind="apps" size={24} />}
          title={app}
          detail={`Runs ${script.command.split('\n').filter((l) => l.trim()).length === 1 ? 'one step' : `${script.command.split('\n').filter((l) => l.trim()).length} steps`} in a terminal window`}
          end={
            confirm === app ? (
              <div className="actions">
                <Link onPress={() => setConfirm(null)}>Cancel</Link>
                <Push
                  onPress={() => {
                    setConfirm(null);
                    s.act(
                      `Installing ${app}`,
                      (onStep) => api.installWithScript(app, onStep),
                      () => s.refresh(),
                    );
                  }}
                >
                  Run It
                </Push>
              </div>
            ) : (
              <Push onPress={() => setConfirm(app)}>Install…</Push>
            )
          }
        >
          {confirm === app && (
            <Caution>
              <b>Run this only if you trust where it came from.</b> It was added to your synced
              settings and runs with your access: it can read, change or delete anything you can.
              The command is: <code>{script.command}</code>
            </Caution>
          )}
        </Row>
      ))}
    </Group>
  );
}

const has = (list: unknown[]) => (list.length === 1 ? 'has' : 'have');

const major = (version?: string) => {
  const n = Number.parseInt(version ?? '', 10);
  return Number.isNaN(n) ? null : n;
};

/**
 * Computers on different Omarchy major versions. The shared Omarchy settings follow the newest;
 * an older computer keeps its own until it's updated, and can try the newer ones with a way back.
 */
function OmarchyVersions({ s }: { s: SyncState }) {
  const [confirm, setConfirm] = useState(false);
  const computers = Object.entries(s.data?.computers ?? {});
  const me = s.data?.computers[s.host];
  const mine = major(me?.omarchy);
  const newest = Math.max(0, ...computers.map(([, c]) => major(c.omarchy) ?? 0));
  if (!me || mine === null || newest === 0) return null;
  const ahead = computers.filter(([, c]) => major(c.omarchy) === newest);
  const behind = computers.filter(
    ([id, c]) => id !== s.host && (major(c.omarchy) ?? newest) < newest,
  );
  const names = (list: typeof computers) => shortList(list.map(([id]) => s.nameOf(id)));

  if (mine < newest && me.tried === newest && !me.paused) {
    return (
      <Group title="Trying Newer Omarchy Settings">
        <Row
          title={`This computer has Omarchy ${me.omarchy} and uses the settings from Omarchy ${newest}.`}
          detail="If something doesn't work, put back the settings it had before. Updating Omarchy here keeps the newer ones for good."
          end={
            <Push
              disabled={!s.idle}
              onPress={() =>
                s.act(
                  'Putting back your Omarchy settings',
                  () => api.putBackOmarchy(),
                  () => s.sync(),
                )
              }
            >
              Put Back My Settings
            </Push>
          }
        />
      </Group>
    );
  }
  if (mine < newest) {
    return (
      <Group
        title="Omarchy Settings Paused"
        footer="Apps, app data, terminals and the background still sync."
      >
        <Row
          title={`This computer has Omarchy ${me.omarchy}; ${names(ahead)} ${has(ahead)} Omarchy ${ahead[0][1].omarchy}.`}
          detail="Settings made for a newer Omarchy may not work on an older one, so this computer keeps its own Omarchy settings for now. Update Omarchy here and they sync again by themselves."
          end={
            <Push disabled={!s.idle} onPress={() => api.updateOmarchy().catch(s.fail)}>
              Update Omarchy
            </Push>
          }
        >
          <div className="inline">
            {confirm ? (
              <>
                <Link onPress={() => setConfirm(false)}>Cancel</Link>
                <Push
                  onPress={() => {
                    setConfirm(false);
                    void s.edit(() => api.tryOmarchy()).then(() => s.sync());
                  }}
                >
                  Try Them
                </Push>
              </>
            ) : (
              <Link onPress={() => setConfirm(true)}>Try Their Settings Anyway…</Link>
            )}
          </div>
          {confirm && (
            <Caution>
              <b>This computer's Omarchy settings are saved first.</b> If something doesn't work
              after the sync, Put Back My Settings restores them exactly and pauses again.
            </Caution>
          )}
        </Row>
      </Group>
    );
  }
  if (behind.length === 0) return null;
  return (
    <Group
      title="Older Omarchy Elsewhere"
      footer="Your Omarchy settings follow the newest Omarchy. Update the older computers, not this one; their Omarchy settings sync again once they're updated."
    >
      {behind.map(([id, c]) => (
        <Row
          key={id}
          title={s.nameOf(id)}
          detail={`Omarchy ${c.omarchy}. ${
            c.tried === newest
              ? 'It is trying the newer settings.'
              : 'It keeps its own Omarchy settings until it is updated.'
          }`}
        />
      ))}
    </Group>
  );
}

export function SyncPane({ s }: { s: SyncState }) {
  const ov = s.overview;
  if (s.conflict) {
    return (
      <>
        <About />
        <Hero s={s} />
        <NoticeLine s={s} />
        <ConflictGroup s={s} />
      </>
    );
  }
  const showWaiting = s.waiting.length > 0 && s.waiting.join() !== s.dismissed && s.idle;
  const pluginIds = s.pluginsWaiting.map((p) => p.id).join();
  const showPlugins = pluginIds !== '' && pluginIds !== s.dismissedPlugins && s.idle;
  const from =
    ov?.incoming_from.length === 1
      ? `From ${s.nameOf(ov.incoming_from[0])}.`
      : 'From your other computers.';
  return (
    <>
      <About />
      <Hero s={s} />
      <NoticeLine s={s} />
      {ov && (
        <>
          <OmarchyVersions s={s} />
          {showWaiting && (
            // Installing changes the system, so it waits for a yes unless this computer opted out of asking.
            <Group title="Apps Waiting to Install" footer="Omarchy will ask for your password.">
              {s.waiting.map((app) => (
                <Row key={app} lead={<Tile icon={Package} kind="apps" size={24} />} title={app} />
              ))}
              <div className="row actions">
                <Push onPress={() => s.setDismissed(s.waiting.join())}>Not Now</Push>
                <Push
                  onPress={() =>
                    s.act(
                      'Installing apps',
                      () => api.installApps(),
                      () => s.refresh(),
                    )
                  }
                >
                  Install
                </Push>
              </div>
            </Group>
          )}
          {ov.missing_repos.length > 0 && (
            <Group title="Repositories to Add">
              <Row
                lead={<Tile icon={Package} kind="apps" size={24} />}
                title={
                  ov.missing_repos.length === 1
                    ? '1 repository'
                    : `${ov.missing_repos.length} repositories`
                }
                detail={`Needed for ${shortList(ov.missing_repos.flatMap((r) => r.apps))}.`}
                end={<Push onPress={() => s.setTab('repos')}>Show Repositories</Push>}
              />
            </Group>
          )}
          {s.scripted.length > 0 && s.idle && <ScriptedApps s={s} />}
          {showPlugins && (
            <Group
              title="Plugins Waiting to Add"
              footer="Your other computers have these. Adding one brings it and its files here."
            >
              {s.pluginsWaiting.map((p) => (
                <Row
                  key={p.id}
                  lead={<Tile icon={Puzzle} kind="plain" size={24} />}
                  title={p.name}
                  detail={`On ${s.whoHas(p.id).join(' and ')}`}
                />
              ))}
              <div className="row actions">
                <Push onPress={() => s.setDismissedPlugins(pluginIds)}>Not Now</Push>
                <Push onPress={() => s.edit(() => api.installPlugins())}>Add</Push>
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
                <Row
                  key={label}
                  title={label}
                  detail={shortList(label === s.catalog.data_label ? ov.choice_changes : files)}
                />
              ))
            )}
          </Group>
          <AddComputer s={s} />
        </>
      )}
    </>
  );
}
