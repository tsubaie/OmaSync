import { type ReactNode, useEffect, useEffectEvent, useState } from 'react';
import {
  AppWindowMac,
  ChevronDown,
  CircleCheck,
  FolderCog,
  History,
  MessageSquareWarning,
  Package,
  Puzzle,
  RefreshCw,
  Server,
  Settings,
  SlidersHorizontal,
  type LucideIcon,
} from 'lucide-react';
import { Button, Tab, TabList, TabPanel, Tabs } from 'react-aria-components';
import { shortList } from './format';
import { AppsPane } from './panes/AppsPane';
import { AppDataPane, OmarchyPane, PluginsPane, pendingForTab } from './panes/FoldersPane';
import { HistoryPane } from './panes/HistoryPane';
import { NoticeLine } from './panes/NoticeLine';
import { ReposPane } from './panes/ReposPane';
import { SettingsPane } from './panes/SettingsPane';
import { Setup } from './panes/Setup';
import { SyncPane } from './panes/SyncPane';
import { THEME_PAGES, ThemePane, useMacosTheme } from './panes/ThemePane';
import { type Tab as TabId, useSync } from './state';
import { api } from './api';
import { Push, Tip } from './ui';

type Section = { id: TabId; name: string; icon: LucideIcon };
type SidebarGroup = Section & { items: Section[] };

// Each group's heading opens its main page and folds away its other pages, like a disclosure in
// a Finder sidebar; Settings sits at the bottom. A new part of the app becomes a new group.
const GROUPS: SidebarGroup[] = [
  {
    id: 'sync',
    name: 'Sync',
    icon: RefreshCw,
    items: [
      { id: 'omarchy', name: 'Configurations', icon: SlidersHorizontal },
      { id: 'plugins', name: 'Plugins', icon: Puzzle },
      { id: 'repos', name: 'Repositories', icon: Server },
      { id: 'apps', name: 'Installed Apps', icon: Package },
      { id: 'data', name: 'App Data', icon: FolderCog },
      { id: 'history', name: 'History', icon: History },
    ],
  },
  { id: 'theme', name: 'macOS Theme', icon: AppWindowMac, items: THEME_PAGES },
];
const SETTINGS: Section = { id: 'settings', name: 'Settings', icon: Settings };
const SECTIONS = [...GROUPS.flatMap((g) => [g, ...g.items]), SETTINGS];

/**
 * The logo: two arrows turning into each other, drawn as vectors so they stay sharp, and the
 * name as text in the app's typeface. The narrow sidebar shows only the arrows.
 */
function Brand() {
  const arrow = 'M6 74V23Q6 8.5 20.5 8.5H57V-2L80 17.25 57 36.5V26H24V74Z';
  return (
    <div className="brand">
      <svg className="mark" viewBox="-3 -3 106 106" aria-hidden="true">
        <path className="one" d={arrow} />
        <path className="two" d={arrow} transform="rotate(180 50 50)" />
      </svg>
      <span className="name">
        Omarchy <span className="sync">Sync</span>
      </span>
    </div>
  );
}

const PANES: Partial<Record<TabId, (p: { s: ReturnType<typeof useSync> }) => ReactNode>> = {
  sync: SyncPane,
  history: HistoryPane,
  omarchy: OmarchyPane,
  plugins: PluginsPane,
  repos: ReposPane,
  apps: AppsPane,
  data: AppDataPane,
  settings: SettingsPane,
};

export function App() {
  const s = useSync();
  const [scrolled, setScrolled] = useState(false);
  const theme = useMacosTheme(s);
  /** The groups folded away. */
  const [folded, setFolded] = useState<TabId[]>([]);
  const fold = (g: SidebarGroup, close: boolean) => {
    setFolded((f) => (close ? [...f, g.id] : f.filter((x) => x !== g.id)));
    // A page folded away can't stay open; its group's main page takes its place.
    if (close && g.items.some((i) => i.id === s.tab)) s.setTab(g.id);
  };
  // Right and Left unfold and fold the group whose heading has focus, as in a Finder sidebar.
  const onKey = useEffectEvent((e: KeyboardEvent) => {
    const g = GROUPS.find((x) => x.id === (e.target as HTMLElement).dataset?.group);
    if (g && (e.key === 'ArrowRight' || e.key === 'ArrowLeft')) fold(g, e.key === 'ArrowLeft');
  });
  useEffect(() => {
    const listener = (e: KeyboardEvent) => onKey(e);
    // Before React Aria's tab list, which keeps arrow keys to itself.
    window.addEventListener('keydown', listener, true);
    return () => window.removeEventListener('keydown', listener, true);
  }, []);
  const select = (id: TabId) => {
    // Opening a group's main page unfolds it.
    setFolded((f) => f.filter((g) => g !== id));
    s.setTab(id);
  };

  if (!s.setup) {
    return (
      <main className="page">
        <div className="setup">
          <output className="loading">
            <span className="dot" />
            Getting ready…
          </output>
          <NoticeLine s={s} />
        </div>
      </main>
    );
  }
  if (!s.ready) {
    return (
      <main className="page">
        <Setup s={s} st={s.setup} />
      </main>
    );
  }

  // A macOS-style source list: the sidebar is a vertical tab list (arrow keys move between sections).
  return (
    <Tabs
      className={`window ${s.busy ? 'busy' : ''}`}
      orientation="vertical"
      selectedKey={s.tab}
      // Until the first check finishes there's nothing to show anywhere but Sync.
      disabledKeys={s.overview ? [] : SECTIONS.filter((x) => x.id !== 'sync').map((x) => x.id)}
      onSelectionChange={(k) => select(k as TabId)}
    >
      <div className="sidebar">
        <Brand />
        <TabList className="sections" aria-label="Sections">
          {[
            ...GROUPS.flatMap((g) => {
              // A page opened by a shortcut or a link shows even in a folded group.
              const open = !folded.includes(g.id) || g.items.some((i) => i.id === s.tab);
              return [
                <Tab
                  key={g.id}
                  id={g.id}
                  className="nav group"
                  data-open={open || undefined}
                  aria-label={g.name}
                  data-group={g.id}
                >
                  {/* Plain line icons in the text colour, as macOS sidebars draw them. */}
                  <g.icon size={16} strokeWidth={1.75} aria-hidden="true" />
                  <span className="label">{g.name}</span>
                  {/* A click here folds or unfolds without opening the page. */}
                  <span
                    className="disclosure"
                    aria-hidden="true"
                    onPointerDown={(e) => e.stopPropagation()}
                    onClick={() => fold(g, open)}
                  >
                    <ChevronDown size={14} strokeWidth={2} />
                  </span>
                </Tab>,
                ...(open ? g.items : []).map((sec) => (
                  <Tab key={sec.id} id={sec.id} className="nav child" aria-label={sec.name}>
                    <sec.icon size={15} strokeWidth={1.75} aria-hidden="true" />
                    <span className="label">{sec.name}</span>
                  </Tab>
                )),
              ];
            }),
            <Tab key={SETTINGS.id} id={SETTINGS.id} className="nav end" aria-label={SETTINGS.name}>
              <SETTINGS.icon size={16} strokeWidth={1.75} aria-hidden="true" />
              <span className="label">{SETTINGS.name}</span>
            </Tab>,
          ]}
        </TabList>
        {/* Beside Settings: report a problem, suggest something, or ask (GitHub issues). */}
        <Tip label="Report a Problem or Suggest Something">
          <Button
            className="report"
            aria-label="Report a Problem or Suggest Something"
            onPress={() => api.report().catch(s.fail)}
          >
            <MessageSquareWarning size={15} strokeWidth={1.75} aria-hidden="true" />
          </Button>
        </Tip>
      </div>
      {SECTIONS.map((sec) => {
        const Pane = PANES[sec.id];
        const pending = pendingForTab(s.pending, sec.id, s.catalog.folders);
        return (
          <TabPanel key={sec.id} id={sec.id} className="pane-wrap">
            <div
              className={`pane ${scrolled ? 'scrolled' : ''}`}
              onScroll={(e) => setScrolled(e.currentTarget.scrollTop > 0)}
            >
              <header className="toolbar">
                <h1>{sec.name}</h1>
              </header>
              <div className="content">
                {pending.length > 0 && s.idle && (
                  <div className="box row">
                    <div className="text">
                      {shortList(pending, 2)}. Your other computers get this the next time you sync.
                    </div>
                    <Push onPress={() => s.sync()}>Sync Now</Push>
                  </div>
                )}
                {sec.id !== 'sync' && <NoticeLine s={s} />}
                {Pane ? <Pane s={s} /> : <ThemePane s={s} t={theme} page={sec.id} />}
              </div>
            </div>
          </TabPanel>
        );
      })}
      {s.toast && (
        <output key={s.toast.id} className="toast">
          <CircleCheck size={16} aria-hidden="true" />
          {s.toast.text}
        </output>
      )}
    </Tabs>
  );
}
