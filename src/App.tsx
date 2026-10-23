import { useState } from 'react';
import { FolderSync, History, Package, RefreshCw, Settings, type LucideIcon } from 'lucide-react';
import { Tab, TabList, TabPanel, Tabs } from 'react-aria-components';
import { AppsPane } from './panes/AppsPane';
import { FoldersPane } from './panes/FoldersPane';
import { HistoryPane } from './panes/HistoryPane';
import { NoticeLine } from './panes/NoticeLine';
import { SettingsPane } from './panes/SettingsPane';
import { Setup } from './panes/Setup';
import { SyncPane } from './panes/SyncPane';
import { type Tab as TabId, useSync } from './state';
import { Push, Tile } from './ui';

const SECTIONS: { id: TabId; name: string; icon: LucideIcon }[] = [
  { id: 'sync', name: 'Sync', icon: RefreshCw },
  { id: 'apps', name: 'Apps', icon: Package },
  { id: 'folders', name: "What's Synced", icon: FolderSync },
  { id: 'history', name: 'History', icon: History },
  { id: 'settings', name: 'Settings', icon: Settings },
];

const PANES = {
  sync: SyncPane,
  apps: AppsPane,
  folders: FoldersPane,
  history: HistoryPane,
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
      onSelectionChange={(k) => s.setTab(k as TabId)}
    >
      <TabList className="sidebar" aria-label="Sections">
        {SECTIONS.map((sec) => (
          <Tab key={sec.id} id={sec.id} className="nav" aria-label={sec.name}>
            <Tile icon={sec.icon} kind={sec.id} />
            <span className="label">{sec.name}</span>
          </Tab>
        ))}
      </TabList>
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
    </Tabs>
  );
}
