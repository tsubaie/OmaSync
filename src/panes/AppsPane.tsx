import { useState } from 'react';
import { Search } from 'lucide-react';
import { Input, SearchField } from 'react-aria-components';
import { api } from '../api';
import type { SyncState } from '../state';
import { Group, Push, Row, Toggle } from '../ui';

/** One list in a fixed order: each app's Sync switch flips in place, and its row never moves. */
export function AppsPane({ s }: { s: SyncState }) {
  const [query, setQuery] = useState('');
  const { overview: ov, data, host } = s;
  if (!ov || !data) return null;
  const computers = [host, ...Object.keys(data.computers ?? {}).filter((c) => c !== host)];
  const name = (c: string) => (c === host ? 'This Computer' : s.nameOf(c));
  const columns = { '--n': computers.length } as React.CSSProperties;
  const apps = [...new Set([...data.apps, ...ov.apps.mine])].toSorted();
  const unsynced = apps.filter((a) => !data.apps.includes(a));
  const q = query.trim().toLowerCase();
  const shown = apps.filter(
    (app) => !q || app.includes(q) || (ov.apps.about[app] ?? '').toLowerCase().includes(q),
  );
  return (
    <>
      <Group
        title="Your Apps"
        footer="A synced app is installed on each switched-on computer after you approve it. Switching anything off never uninstalls an app."
      >
        {apps.length > 6 && (
          <div className="row">
            <SearchField
              className="field search"
              aria-label="Search your apps"
              value={query}
              onChange={setQuery}
            >
              <Search size={14} aria-hidden="true" />
              <Input placeholder="Search" />
            </SearchField>
          </div>
        )}
        {apps.length === 0 ? (
          <Row title="You haven't installed any apps of your own yet." />
        ) : shown.length === 0 ? (
          <Row title={`No apps match “${query.trim()}”.`} />
        ) : (
          <>
            <div className="row matrix head" style={columns} aria-hidden="true">
              <span>App</span>
              <span>Sync</span>
              {computers.map((c) => (
                <span key={c}>{name(c)}</span>
              ))}
            </div>
            {shown.map((app) => {
              const synced = data.apps.includes(app);
              const about = ov.apps.about[app];
              const missingHere = synced && s.wants(host, app) && !ov.apps.installed.includes(app);
              return (
                <div className="row matrix" style={columns} key={app}>
                  <div className="text">
                    <div>{app}</div>
                    {about ? (
                      <div className="detail">{about}</div>
                    ) : (
                      missingHere && (
                        <div className="detail warn">Installs here after you approve it</div>
                      )
                    )}
                  </div>
                  <div className="cell">
                    <span className="cell-label">Sync</span>
                    <Toggle
                      on={synced}
                      label={`Sync ${app}`}
                      disabled={!s.idle}
                      onChange={(on) => s.edit(() => (on ? api.addApps([app]) : api.stopApp(app)))}
                    />
                  </div>
                  {computers.map((c) => (
                    <div className="cell" key={c}>
                      {synced && (
                        <>
                          <span className="cell-label">{name(c)}</span>
                          <Toggle
                            on={s.wants(c, app)}
                            label={`${app} on ${name(c)}`}
                            disabled={!s.idle}
                            onChange={(on) => s.edit(() => api.setApp(c, app, on))}
                          />
                        </>
                      )}
                    </div>
                  ))}
                </div>
              );
            })}
          </>
        )}
      </Group>
      {unsynced.length > 1 && (
        <div className="actions">
          {/* One change for all of them, shared on the next sync. */}
          <Push disabled={!s.idle} onPress={() => s.edit(() => api.addApps(unsynced))}>
            Sync All {unsynced.length} Apps
          </Push>
        </div>
      )}
    </>
  );
}
