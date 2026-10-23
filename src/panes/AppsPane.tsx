import { useState } from 'react';
import { Search } from 'lucide-react';
import { Input, SearchField } from 'react-aria-components';
import { api } from '../api';
import type { SyncState } from '../state';
import { Group, Link, Push, Row, Toggle } from '../ui';

export function AppsPane({ s }: { s: SyncState }) {
  const [query, setQuery] = useState('');
  const { overview: ov, data, host } = s;
  if (!ov || !data) return null;
  const computers = [host, ...Object.keys(data.computers ?? {}).filter((c) => c !== host)];
  const name = (c: string) => (c === host ? 'This Computer' : s.nameOf(c));
  const columns = { '--n': computers.length } as React.CSSProperties;
  const local = ov.apps.local_only;
  const q = query.trim().toLowerCase();
  const shown = local.filter(
    (app) => !q || app.includes(q) || (ov.apps.about[app] ?? '').toLowerCase().includes(q),
  );
  return (
    <>
      <Group
        title="Synced Apps"
        footer="A switched-on app is installed on that computer after you approve it. Switching it off only stops that; nothing is ever uninstalled."
      >
        {data.apps.length === 0 ? (
          <Row title="No apps are synced yet." detail="Pick some from the list below." />
        ) : (
          <>
            <div className="row matrix head" style={columns} aria-hidden="true">
              <span>App</span>
              {computers.map((c) => (
                <span key={c}>{name(c)}</span>
              ))}
              <span />
            </div>
            {data.apps.map((app) => {
              const about = ov.apps.about[app];
              const missingHere = s.wants(host, app) && !ov.apps.installed.includes(app);
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
                  {computers.map((c) => (
                    <div className="cell" key={c}>
                      <span className="cell-label">{name(c)}</span>
                      <Toggle
                        on={s.wants(c, app)}
                        label={`${app} on ${name(c)}`}
                        disabled={!s.idle}
                        onChange={(on) => s.edit(() => api.setApp(c, app, on))}
                      />
                    </div>
                  ))}
                  <Link disabled={!s.idle} onPress={() => s.edit(() => api.stopApp(app))}>
                    Stop Syncing
                  </Link>
                </div>
              );
            })}
          </>
        )}
      </Group>
      <Group
        title="Only on This Computer"
        footer="Apps you installed here that your other computers don't have."
      >
        {local.length > 6 && (
          <div className="row">
            <SearchField
              className="field search"
              aria-label="Search these apps"
              value={query}
              onChange={setQuery}
            >
              <Search size={14} aria-hidden="true" />
              <Input placeholder="Search" />
            </SearchField>
          </div>
        )}
        {local.length === 0 ? (
          <Row title="Every app you installed yourself is synced." />
        ) : shown.length === 0 ? (
          <Row title={`No apps match “${query.trim()}”.`} />
        ) : (
          shown.map((app) => (
            <Row
              key={app}
              title={app}
              detail={ov.apps.about[app]}
              end={
                <Push disabled={!s.idle} onPress={() => s.edit(() => api.addApps([app]))}>
                  Sync to All
                </Push>
              }
            />
          ))
        )}
      </Group>
      {local.length > 1 && (
        <div className="actions">
          <Push disabled={!s.idle} onPress={() => s.edit(() => api.addApps(local))}>
            Sync All {local.length} Apps
          </Push>
        </div>
      )}
    </>
  );
}
