// All of the window's state and actions. Real work happens in Rust; this keeps track of what
// to show and runs one job at a time, with its current step.

import { useEffect, useEffectEvent, useRef, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  api,
  type Catalog,
  type Conflict,
  type Data,
  type Prefer,
  type Problem,
  type SetupState,
  type Status,
} from './api';
import { applyLook } from './theme';

export type Tab = 'sync' | 'history' | 'omarchy' | 'plugins' | 'apps' | 'data' | 'settings';
export const TABS: Tab[] = ['sync', 'history', 'omarchy', 'plugins', 'apps', 'data', 'settings'];

export type Notice = { good: boolean; text: string; details?: string };

export function useSync() {
  const [catalog, setCatalog] = useState<Catalog>({ folders: [], data_label: '' });
  const [setup, setSetup] = useState<SetupState | null>(null);
  const [tab, setTab] = useState<Tab>('sync');
  // The window opens already checking what state this computer is in.
  const [busy, setBusy] = useState<string | null>('Getting ready');
  const [syncing, setSyncing] = useState(false);
  const [status, setStatus] = useState<Status | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [conflict, setConflict] = useState<Conflict | null>(null);
  const [confirmUndo, setConfirmUndo] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState('');
  const [dismissedPlugins, setDismissedPlugins] = useState('');
  const [signingIn, setSigningIn] = useState(false);
  const [pending, setPending] = useState(false);
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
    if (p?.kind === 'Conflict') setConflict({ files: p.files, by: p.by });
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
  ): Promise<T | undefined> {
    if (working.current) return undefined;
    working.current = true;
    setBusy(first);
    try {
      return await work(setBusy);
    } finally {
      working.current = false;
      setBusy(null);
      setSyncing(false);
    }
  }

  async function refresh() {
    try {
      const st = await run('Checking for changes', (onStep) => api.overview(onStep));
      if (!st) return;
      setStatus(st);
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

  async function probe() {
    try {
      const s = await run('Getting ready', () => api.probe());
      if (s) await show(s);
    } catch (e) {
      fail(e);
    }
  }

  async function show(s: SetupState) {
    setSetup(s);
    if (s.ready) await refresh();
  }

  /** Syncs (or undoes a change, then syncs), then shows the fresh state. */
  async function sync(prefer: Prefer = 'Ask', undoSha?: string) {
    if (working.current) return;
    setNotice(null);
    setTab('sync');
    setSyncing(true);
    try {
      const report = await run(undoSha ? 'Undoing' : 'Starting', (onStep) =>
        undoSha ? api.undo(undoSha, onStep) : api.syncNow(prefer, onStep),
      );
      if (report) {
        // Success is a small toast; anything worth reading (apps installed or not) stays on screen.
        if (report.notes.length) setNotice({ good: true, text: report.notes.join(' ') });
        const id = Date.now();
        setToast({ id, text: undoSha ? 'Undone' : 'Synced' });
        setTimeout(() => setToast((t) => (t?.id === id ? null : t)), 3000);
        setConflict(null);
        setPending(false);
      }
    } catch (e) {
      fail(e);
    }
    await refresh();
  }

  async function act<T>(
    label: string,
    work: () => Promise<T>,
    after?: (result: T) => Promise<void> | void,
  ) {
    try {
      const result = await run(label, work);
      if (result !== undefined && after) await after(result);
    } catch (e) {
      fail(e);
    }
  }

  /** Small edits to the shared app and folder choices: saved now, shared on the next sync. */
  async function edit(work: () => Promise<Data>) {
    try {
      const data = await work();
      setStatus((s) => (s ? { ...s, data } : s));
      // Compared with what was last shared, so flipping a switch back hides the banner again.
      setPending(await api.choicesChanged());
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

  // Each minute: move the clock on, and every five quietly look for news from the other computers.
  const onTick = useEffectEvent(() => {
    setNow(Math.floor(Date.now() / 1000));
    if (ready && !working.current && Date.now() - lastRefresh.current >= 5 * 60 * 1000) refresh();
  });
  useEffect(() => {
    const timer = setInterval(onTick, 60 * 1000);
    return () => clearInterval(timer);
  }, []);

  // Ctrl+Q quits, Ctrl+R or F5 syncs (instead of reloading the page), Ctrl+1 to 7 switch panes.
  const onKey = useEffectEvent((e: KeyboardEvent) => {
    const ctrl = e.ctrlKey && !e.altKey && !e.metaKey;
    if (ctrl && e.key === 'q') getCurrentWindow().close();
    if ((ctrl && e.key === 'r') || e.key === 'F5') {
      e.preventDefault();
      if (ready && !conflict) sync();
    }
    // Like the sidebar, only Sync opens before the first check finishes.
    if (ctrl && ready && e.key >= '1' && e.key <= (status ? '7' : '1')) {
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
  const wants = (computer: string, app: string) =>
    !!data?.apps.includes(app) && !(data.computers?.[computer]?.skip_apps ?? []).includes(app);
  const waiting =
    overview && data
      ? data.apps.filter((a) => wants(host, a) && !overview.apps.installed.includes(a))
      : [];
  const wantsPlugin = (computer: string, plugin: string) =>
    !!data?.plugins?.includes(plugin) &&
    !(data.computers?.[computer]?.skip_plugins ?? []).includes(plugin);
  /** The names of the other computers that have a plugin. */
  const whoHas = (plugin: string) =>
    Object.entries(data?.computers ?? {})
      .filter(([id, c]) => id !== host && (c.plugins ?? []).includes(plugin))
      .map(([id]) => nameOf(id));
  // Mirrors Data::missing_plugins in sync.rs: other computers' plugins this one doesn't have.
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
    setSetup,
    setNotice,
    now,
    checked,
    setup,
    ready,
    tab,
    setTab,
    busy,
    idle: busy === null,
    syncing,
    notice,
    conflict,
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
    pluginsWaiting,
    whoHas,
    wantsPlugin,
    dismissedPlugins,
    setDismissedPlugins,
    fail,
    probe,
    refresh,
    sync,
    act,
    edit,
  };
}

export type SyncState = ReturnType<typeof useSync>;
