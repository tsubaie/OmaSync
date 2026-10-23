import { useState } from 'react';
import { Search } from 'lucide-react';
import { Input, Label, SearchField, Text, TextField } from 'react-aria-components';
import { api } from '../api';
import type { SyncState } from '../state';
import { Caution, Group, Link, Push, Row, Toggle } from '../ui';

/**
 * How to install an app that isn't in Arch's repos or the AUR: one command, run in an empty
 * folder in a terminal window. It starts empty; the person types it in.
 */
function ScriptForm({ s, app, onDone }: { s: SyncState; app: string; onDone: () => void }) {
  const [command, setCommand] = useState(s.data?.outside?.[app]?.command ?? '');
  const save = () =>
    s.edit(async () => {
      const data = await api.setAppScript(app, command);
      onDone();
      return data;
    });
  return (
    <div className="script-form">
      <Caution>
        <b>Only enter a command from a source you fully trust.</b> It runs on every computer that
        installs {app}, with your access: it can read, change or delete anything you can. Never
        paste a command from someone you don't know.
      </Caution>
      <TextField className="field labelled" value={command} onChange={setCommand} isRequired>
        <Label>Install command</Label>
        <Input />
        <Text slot="description">
          Runs in an empty folder, in a terminal window you can see. It can download the app itself,
          for example with git clone or an installer from the app's website.
        </Text>
      </TextField>
      <div className="actions">
        <Link onPress={onDone}>Cancel</Link>
        <Push disabled={!s.idle || !command.trim()} onPress={save}>
          Save Install Command
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
  // Sync All leaves out apps that can't sync yet (no install command).
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
              // Not in Arch's repos or the AUR: it syncs only with its install command.
              const outside = ov.apps.outside.includes(app) || app in (data.outside ?? {});
              const script = data.outside?.[app] ?? null;
              return (
                <div className="row matrix" style={columns} key={app}>
                  <div className="text">
                    <div>{app}</div>
                    {missingHere ? (
                      <div className="detail warn">
                        {script
                          ? 'Installs here with its install command'
                          : 'Installs here after you approve it'}
                      </div>
                    ) : (
                      <div className="detail">{synced ? about : 'Only on this computer'}</div>
                    )}
                    {outside && (
                      <div className="detail">
                        {script
                          ? `Not in Arch's repos or the AUR. Installs with “${script.command}”.`
                          : "Not in Arch's repos or the AUR. Add its install command to sync it."}
                      </div>
                    )}
                    {outside && editing !== app && (
                      <div className="inline">
                        <Link disabled={!s.idle} onPress={() => setEditing(app)}>
                          {script ? 'Edit Install Command…' : 'Add an Install Command…'}
                        </Link>
                        {script && (
                          <Link
                            disabled={!s.idle}
                            onPress={() => s.edit(() => api.removeAppScript(app))}
                          >
                            Remove Install Command
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
                        disabled={!s.idle || (outside && !script)}
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
