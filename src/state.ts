// All of the window's state and actions. Real work happens in Rust; this keeps track of what
// to show and runs one job at a time, with its current step.

import { useEffect, useEffectEvent, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  api,
  type Catalog,
  type Conflict,
  type Data,
  type Keep,
  type Problem,
  type Report,
  type SetupState,
  type Snapshot,
  type Status,
} from './api';
import { applyLook } from './theme';

export type Tab =
  'sync' | 'history' | 'omarchy' | 'plugins' | 'repos' | 'apps' | 'data' | 'settings';
export const TABS: Tab[] = [
  'sync',
  'omarchy',
  'plugins',
  'repos',
  'apps',
  'data',
  'history',
  'settings',
];

/** `warn`: it worked, with something to know (shown as a warning, not as a success). */
export type Notice = { good: boolean; warn?: boolean; text: string; details?: string };

/** What a finished change to the system says, with how the snapshot before it went. */
export function snapshotNotice(text: string, snapshot: Snapshot | null): Notice {
  if (typeof snapshot === 'object' && snapshot && 'Taken' in snapshot)
    return {
      good: true,
      text: `${text} Snapshot ${snapshot.Taken} of this computer's system was taken just before; to go back to it, restart and pick it from the startup menu.`,
    };
  if (typeof snapshot === 'object' && snapshot && 'Failed' in snapshot)
    return {
      good: true,
      warn: true,
      text: `${text} A snapshot couldn't be taken first.`,
      details: snapshot.Failed,
    };
  return { good: true, text };
}

/** Where a job sits in a round of progress (see run). */
type Round = 'whole' | 'first' | 'middle' | 'last';

export function useSync() {
  const [catalog, setCatalog] = useState<Catalog>({ folders: [], data_label: '' });
  const [setup, setSetup] = useState<SetupState | null>(null);
  const [tab, setTab] = useState<Tab>('sync');
  // The window opens already checking what state this computer is in.
  const [busy, setBusy] = useState<string | null>('Getting ready');
  /** The steps the running job has reached so far, the last one in progress. */
  const [steps, setSteps] = useState<string[]>([]);
  /** The steps it announced it will take (some reached steps may be extra, like new files). */
  const [plan, setPlan] = useState<string[]>([]);
  /** How far along the current step is, when it can tell (uploads and downloads). */
  const [percent, setPercent] = useState<number | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [status, setStatus] = useState<Status | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [conflict, setConflict] = useState<Conflict | null>(null);
  const [confirmUndo, setConfirmUndo] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState('');
  const [dismissedPlugins, setDismissedPlugins] = useState('');
  const [signingIn, setSigningIn] = useState(false);
  /** What changed in the choices since the last sync, in sentences; none when nothing. */
  const [pending, setPending] = useState<string[]>([]);
  const [toast, setToast] = useState<{ id: number; text: string } | null>(null);
  const working = useRef(true);
  // Unix seconds, ticking each minute; components read it instead of the clock so rendering stays pure.
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const lastRefresh = useRef(0);
  /** When this window last looked for changes, as unix time. */
  const [checked, setChecked] = useState<number | null>(null);

  const ready = !!setup?.ready;

  function fail(e: unknown) {
    const p = e as Problem;
    if (p?.kind === 'Conflict')
      setConflict({ files: p.files, by: p.by, versions: p.versions, broken: p.broken });
    else if (p?.kind === 'Failed') setNotice({ good: false, text: p.message, details: p.details });
    else
      setNotice({
        good: false,
        text: 'Something went wrong. Nothing on this computer was lost.',
        details: String(e),
      });
  }

  /** Runs one piece of work at a time, showing its current step. */
  async function run<T>(
    first: string,
    work: (onStep: (s: string) => void) => Promise<T>,
    // Setup, a sync and the look afterwards show as one round of progress: its first part
    // starts the step list, a middle part carries it on, and its last part clears it, as a
    // job on its own does. The list must never outlive the round, or it looks frozen.
    round: Round = 'whole',
  ): Promise<T | undefined> {
    const starts = round === 'whole' || round === 'first';
    const ends = round === 'whole' || round === 'last';
    if (working.current) {
      if (ends && !starts) endRound();
      return undefined;
    }
    working.current = true;
    setBusy(first);
    if (starts) {
      setSteps([]);
      setPlan([]);
    }
    setPercent(null);
    try {
      return await work((line) => {
        // Besides step names, ctx.rs sends the plan and percentages on lines starting \u0001.
        const [kind, ...rest] = line.split('\t');
        if (kind === '\u0001plan') setPlan(rest);
        else if (kind === '\u0001percent') setPercent(Number(rest[0]));
        else {
          setBusy(line);
          setSteps((done) => [...done, line]);
          setPercent(null);
        }
      });
    } finally {
      working.current = false;
      setBusy(null);
      setPercent(null);
      if (ends) endRound();
    }
  }

  /** A small confirmation that fades after a moment, for a success with nothing more to say. */
  function flash(text: string) {
    const id = Date.now();
    setToast({ id, text });
    setTimeout(() => setToast((t) => (t?.id === id ? null : t)), 3000);
  }

  function endRound() {
    setSteps([]);
    setPlan([]);
    setSyncing(false);
  }

  async function refresh(afterSync = false) {
    try {
      const st = await run(
        'Checking for changes',
        (onStep) => api.overview(onStep, afterSync),
        afterSync ? 'last' : 'whole',
      );
      if (!st) return;
      setStatus(st);
      // Changes already on disk (from an earlier session) show in their tab's banner too.
      setPending(st.overview?.choice_changes ?? []);
      lastRefresh.current = Date.now();
      setNow(Math.floor(Date.now() / 1000));
      setChecked(Math.floor(Date.now() / 1000));
      if (st.schedule_error) {
        setNotice({
          good: false,
          text: "Automatic syncing couldn't be turned on, so sync with the button for now.",
          details: st.schedule_error,
        });
      }
    } catch (e) {
      fail(e);
    }
  }

  /** After setup: straight into the first sync, carrying on setup's progress in one round. */
  async function finishSetup() {
    try {
      const st = await api.probe();
      setSetup(st);
      if (st.ready) await sync(undefined, true);
      else endRound();
    } catch (e) {
      endRound();
      fail(e);
    }
  }

  /** Looks again at what setup needs; returns what it found (undefined when busy or failed). */
  async function probe() {
    try {
      const s = await run('Getting ready', () => api.probe());
      if (s) await show(s);
      return s;
    } catch (e) {
      fail(e);
    }
  }

  async function show(s: SetupState) {
    setSetup(s);
    if (s.ready) await refresh();
  }

  /**
   * Syncs; or undoes a change, then syncs; or, with `choices`, settles a clash by keeping the
   * chosen version of each clashing file, then syncs. Then shows the fresh state.
   * `continues`: carry on the progress of setup, which ends by syncing.
   */
  async function sync(undoSha?: string, continues = false, choices?: Record<string, Keep>) {
    if (working.current) return;
    setNotice(null);
    setTab('sync');
    setSyncing(true);
    let report: Report | undefined;
    try {
      report = await run(
        undoSha ? 'Undoing' : 'Starting',
        (onStep) =>
          choices
            ? api.resolveConflict(choices, onStep)
            : undoSha
              ? api.undo(undoSha, onStep)
              : api.syncNow(onStep),
        continues ? 'middle' : 'first',
      );
      if (report) {
        // Success is a small toast; anything worth reading (apps installed or not) stays on screen.
        if (report.notes.length) setNotice(snapshotNotice(report.notes.join(' '), report.snapshot));
        flash(undoSha ? 'Undone' : 'Synced');
        setConflict(null);
        setPending([]);
      }
    } catch (e) {
      fail(e);
    }
    // After a sync, a quick look continues its progress; after a failure, a full check.
    await refresh(!!report);
  }

  async function act<T>(
    label: string,
    work: (onStep: (s: string) => void) => Promise<T>,
    after?: (result: T) => unknown,
    round: 'whole' | 'first' = 'whole',
  ) {
    try {
      const result = await run(label, work, round);
      if (result !== undefined && after) await after(result);
    } catch (e) {
      fail(e);
    }
  }

  /** Small edits to the shared app and folder choices: saved now, shared on the next sync. */
  async function edit(work: () => Promise<Data>) {
    try {
      const data = await work();
      // Compared with what was last shared, so flipping a switch back hides the banner again.
      const changes = await api.choicesChanged();
      setPending(changes);
      // The Sync page counts the choices among the changes to share straight away, as the next
      // look would, rather than saying everything matches until then.
      setStatus((s) => {
        if (!s?.overview) return s ? { ...s, data } : s;
        const rest = s.overview.unshared.filter((u) => u !== catalog.data_label);
        const unshared = changes.length ? [...rest, catalog.data_label] : rest;
        return { ...s, data, overview: { ...s.overview, unshared, choice_changes: changes } };
      });
    } catch (e) {
      fail(e);
    }
  }

  const onStart = useEffectEvent(() => {
    api.look().then(applyLook, fail);
    api.folders().then(setCatalog, fail);
    const done = () => {
      working.current = false;
      setBusy(null);
    };
    api.probe().then(
      (s) => {
        done();
        return show(s);
      },
      (e) => {
        done();
        fail(e);
      },
    );
  });
  useEffect(() => onStart(), []);

  // Opened again (from the launcher or the bar indicator): show the Sync page, freshly checked.
  const onShow = useEffectEvent(() => {
    if (!ready) return;
    setTab('sync');
    if (!working.current) refresh();
  });
  // The Omarchy theme or background changed: recolour, and list the change to share.
  const onLook = useEffectEvent(() => {
    api.look().then(applyLook, fail);
    if (ready && !working.current) refresh();
  });
  useEffect(() => {
    const stop = listen('look-changed', onLook);
    return () => {
      stop.then((unlisten) => unlisten());
    };
  }, []);
  useEffect(() => {
    const stop = listen('show-sync', onShow);
    return () => {
      stop.then((unlisten) => unlisten());
    };
  }, []);

  // Each minute: move the clock on, and every five quietly look for news from the other computers.
  const onTick = useEffectEvent(() => {
    setNow(Math.floor(Date.now() / 1000));
    if (ready && !working.current && Date.now() - lastRefresh.current >= 5 * 60 * 1000) refresh();
  });
  useEffect(() => {
    const timer = setInterval(onTick, 60 * 1000);
    return () => clearInterval(timer);
  }, []);

  // Ctrl+Q quits, Ctrl+R or F5 syncs (instead of reloading the page), Ctrl+1 to 8 switch panes.
  const onKey = useEffectEvent((e: KeyboardEvent) => {
    const ctrl = e.ctrlKey && !e.altKey && !e.metaKey;
    if (ctrl && e.key === 'q') getCurrentWindow().close();
    if ((ctrl && e.key === 'r') || e.key === 'F5') {
      e.preventDefault();
      if (ready && !conflict) sync();
    }
    // Like the sidebar, only Sync opens before the first check finishes.
    if (ctrl && ready && e.key >= '1' && e.key <= (status ? '8' : '1')) {
      e.preventDefault();
      setTab(TABS[Number(e.key) - 1]);
    }
    if (e.key === 'Escape') setConfirmUndo(null);
  });
  useEffect(() => {
    const listener = (e: KeyboardEvent) => onKey(e);
    window.addEventListener('keydown', listener);
    return () => window.removeEventListener('keydown', listener);
  }, []);

  const host = status?.host ?? '';
  const data = status?.data;
  const overview = status?.overview;
  const me = data?.computers?.[host] ?? {};
  /** A computer's name for people; falls back to its ID for computers that never named themselves. */
  const nameOf = (id: string) => data?.computers?.[id]?.name || id;
  // An app from outside the repos without an install script isn't synced, whatever the list says.
  const wants = (computer: string, app: string) =>
    !!data?.apps.includes(app) &&
    data.outside?.[app] !== null &&
    !(data.computers?.[computer]?.skip_apps ?? []).includes(app);
  // Mirrors missing_apps and missing_scripted in apps.rs: yay installs the first kind; apps
  // from outside the repos come only by their script.
  const missing =
    overview && data
      ? data.apps.filter((a) => wants(host, a) && !overview.apps.installed.includes(a))
      : [];
  const waiting = missing.filter((a) => !(a in (data?.outside ?? {})));
  const scripted = missing.flatMap((a) => {
    const script = data?.outside?.[a];
    return script ? [{ app: a, script }] : [];
  });
  const wantsPlugin = (computer: string, plugin: string) =>
    !!data?.plugins?.includes(plugin) &&
    !(data.computers?.[computer]?.skip_plugins ?? []).includes(plugin);
  /** The names of the other computers that have a plugin. */
  const whoHas = (plugin: string) =>
    Object.entries(data?.computers ?? {})
      .filter(([id, c]) => id !== host && (c.plugins ?? []).includes(plugin))
      .map(([id]) => nameOf(id));
  // Mirrors Data::missing_plugins in choices.rs: other computers' plugins this one doesn't have.
  const pluginsWaiting =
    data?.folders.includes('.config/omarchy/plugins') && overview
      ? overview.plugins.filter(
          (p) =>
            wantsPlugin(host, p.id) &&
            !(me.plugins ?? []).includes(p.id) &&
            whoHas(p.id).length > 0,
        )
      : [];

  return {
    catalog,
    toast,
    nameOf,
    syncSpace: status?.sync_space ?? null,
    barIndicator: !!status?.bar_indicator,
    version: status?.version ?? '',
    installedHow: status?.installed_how ?? 'Package',
    setSetup,
    setNotice,
    /** Shows what a finished change to the system says, with its snapshot. */
    changed: (text: string, snapshot: Snapshot) => setNotice(snapshotNotice(text, snapshot)),
    now,
    checked,
    setup,
    ready,
    tab,
    setTab,
    busy,
    steps,
    plan,
    percent,
    idle: busy === null,
    syncing,
    notice,
    conflict,
    setConflict,
    confirmUndo,
    setConfirmUndo,
    dismissed,
    setDismissed,
    signingIn,
    setSigningIn,
    pending,
    host,
    data,
    overview,
    me,
    wants,
    waiting,
    scripted,
    pluginsWaiting,
    whoHas,
    wantsPlugin,
    dismissedPlugins,
    setDismissedPlugins,
    fail,
    flash,
    probe,
    finishSetup,
    refresh,
    sync,
    act,
    edit,
  };
}

export type SyncState = ReturnType<typeof useSync>;
