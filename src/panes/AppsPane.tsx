import { useState } from 'react';
import { Search } from 'lucide-react';
import { Input, Label, SearchField, Text, TextField } from 'react-aria-components';
import { api } from '../api';
import type { SyncState } from '../state';
import { Group, Link, Push, Row, Toggle } from '../ui';

/**
 * How to install an app that isn't in Arch's repos or the AUR: a Git repository and a command
 * to run in it. Both start empty; the person fills them in.
 */
function ScriptForm({ s, app, onDone }: { s: SyncState; app: string; onDone: () => void }) {
  const current = s.data?.outside?.[app];
  const [repo, setRepo] = useState(current?.repo ?? '');
  const [command, setCommand] = useState(current?.command ?? '');
  const save = () =>
    s.edit(async () => {
      const data = await api.setAppScript(app, repo, command);
      onDone();
      return data;
    });
  return (
    <div className="script-form">
      <TextField className="field labelled" value={repo} onChange={setRepo} isRequired>
        <Label>Git repository</Label>
        <Input />
        <Text slot="description">The web address of the app's source, starting with https://.</Text>
      </TextField>
      <TextField className="field labelled" value={command} onChange={setCommand} isRequired>
        <Label>Command to run in it</Label>
        <Input />
        <Text slot="description">
          Runs inside the downloaded repository, in a terminal window you can see.
        </Text>
      </TextField>
      <div className="actions">
        <Link onPress={onDone}>Cancel</Link>
        <Push disabled={!s.idle || !repo.trim() || !command.trim()} onPress={save}>
          Save Install Script
        </Push>
      </div>
    </div>
  );
}

/** One list in a fixed order, one switch per computer; rows never move. */
export function AppsPane({ s }: { s: SyncState }) {
  const [query, setQuery] = useState('');
  const [editing, setEditing] = useState<string | null>(null);
  const { overview: ov, data, host } = s;
  if (!ov || !data) return null;
  const computers = [host, ...Object.keys(data.computers ?? {}).filter((c) => c !== host)];
  const name = (c: string) => (c === host ? 'This Computer' : s.nameOf(c));
  // This computer's column is simply "Sync"; the others are named after their computer.
  const column = (c: string) => (c === host ? 'Sync' : s.nameOf(c));
  const columns = { '--n': computers.length } as React.CSSProperties;
  const apps = [...new Set([...data.apps, ...ov.apps.mine])].toSorted();
  // Sync All leaves out apps that can't reach other computers yet (no install script).
  const needsScript = (a: string) =>
    (ov.apps.outside.includes(a) || a in (data.outside ?? {})) && !data.outside?.[a];
  const unsynced = apps.filter((a) => !data.apps.includes(a) && !needsScript(a));
  const q = query.trim().toLowerCase();
  const shown = apps.filter(
    (app) => !q || app.includes(q) || (ov.apps.about[app] ?? '').toLowerCase().includes(q),
  );
  return (
    <>
      <Group
        title="Your Apps"
        footer="Switch an app on for a computer to have it there; it installs after you approve it. Switching it off never uninstalls it."
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
            {computers.length > 1 && (
              <div className="row matrix head" style={columns} aria-hidden="true">
                <span>App</span>
                {computers.map((c) => (
                  <span key={c}>{column(c)}</span>
                ))}
              </div>
            )}
            {shown.map((app) => {
              const synced = data.apps.includes(app);
              const about = ov.apps.about[app];
              const missingHere = synced && s.wants(host, app) && !ov.apps.installed.includes(app);
              // Not in Arch's repos or the AUR: it reaches other computers only by its script.
              const outside = ov.apps.outside.includes(app) || app in (data.outside ?? {});
              const script = data.outside?.[app] ?? null;
              return (
                <div className="row matrix" style={columns} key={app}>
                  <div className="text">
                    <div>{app}</div>
                    {missingHere ? (
                      <div className="detail warn">
                        {script
                          ? 'Installs here with its script'
                          : 'Installs here after you approve it'}
                      </div>
                    ) : (
                      <div className="detail">{synced ? about : 'Only on this computer'}</div>
                    )}
                    {outside && (
                      <div className="detail">
                        {script
                          ? `Not in Arch's repos or the AUR. Installs with “${script.command}” from ${script.repo}.`
                          : "Not in Arch's repos or the AUR, so other computers need an install script."}
                      </div>
                    )}
                    {outside && editing !== app && (
                      <div className="inline">
                        <Link disabled={!s.idle} onPress={() => setEditing(app)}>
                          {script ? 'Edit Install Script…' : 'Add an Install Script…'}
                        </Link>
                        {script && (
                          <Link
                            disabled={!s.idle}
                            onPress={() => s.edit(() => api.removeAppScript(app))}
                          >
                            Remove Script
                          </Link>
                        )}
                      </div>
                    )}
                    {editing === app && (
                      <ScriptForm s={s} app={app} onDone={() => setEditing(null)} />
                    )}
                  </div>
                  {computers.map((c) => (
                    <div className="cell" key={c}>
                      <span className="cell-label">{column(c)}</span>
                      <Toggle
                        on={s.wants(c, app)}
                        label={`${app} synced on ${name(c)}`}
                        disabled={!s.idle || (outside && !script && c !== host)}
                        onChange={(on) => s.edit(() => api.setApp(c, app, on))}
                      />
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
            Sync All {unsynced.length} Apps to Every Computer
          </Push>
        </div>
      )}
    </>
  );
}
