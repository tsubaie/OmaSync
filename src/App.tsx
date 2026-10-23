import { useState } from 'react';
import {
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
import { AppsPane } from './panes/AppsPane';
import { AppDataPane, OmarchyPane, PluginsPane } from './panes/FoldersPane';
import { HistoryPane } from './panes/HistoryPane';
import { NoticeLine } from './panes/NoticeLine';
import { ReposPane } from './panes/ReposPane';
import { SettingsPane } from './panes/SettingsPane';
import { Setup } from './panes/Setup';
import { SyncPane } from './panes/SyncPane';
import { type Tab as TabId, useSync } from './state';
import { api } from './api';
import { Push, Tip } from './ui';

// Grouped like a Finder sidebar: a quiet heading starts each group; History and Settings sit at
// the bottom.
const SECTIONS: { id: TabId; name: string; icon: LucideIcon; heading?: string; end?: boolean }[] = [
  { id: 'sync', name: 'Sync', icon: RefreshCw },
  { id: 'omarchy', name: 'Configurations', icon: SlidersHorizontal, heading: 'Omarchy' },
  { id: 'plugins', name: 'Plugins', icon: Puzzle },
  { id: 'repos', name: 'Repositories', icon: Server, heading: 'Apps' },
  { id: 'apps', name: 'Installed Apps', icon: Package },
  { id: 'data', name: 'App Data', icon: FolderCog },
  { id: 'history', name: 'History', icon: History, end: true },
  { id: 'settings', name: 'Settings', icon: Settings },
];

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

const PANES = {
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
      onSelectionChange={(k) => s.setTab(k as TabId)}
    >
      <div className="sidebar">
        <Brand />
        <TabList className="sections" aria-label="Sections">
          {SECTIONS.map((sec) => (
            <Tab
              key={sec.id}
              id={sec.id}
              className={`nav ${sec.end ? 'end' : ''}`}
              data-heading={sec.heading}
              aria-label={sec.name}
            >
              {/* Plain line icons in the text colour, as macOS sidebars draw them. */}
              <sec.icon size={16} strokeWidth={1.75} aria-hidden="true" />
              <span className="label">{sec.name}</span>
            </Tab>
          ))}
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
                {s.pending && s.idle && s.tab !== 'sync' && (
                  <div className="box row">
                    <div className="text">
                      Your other computers get this change the next time you sync.
                    </div>
                    <Push onPress={() => s.sync()}>Sync Now</Push>
                  </div>
                )}
                {sec.id !== 'sync' && <NoticeLine s={s} />}
                <Pane s={s} />
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
