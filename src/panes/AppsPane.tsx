import { useState } from 'react';
import { Search } from 'lucide-react';
import { Input, Label, SearchField, Text, TextArea, TextField } from 'react-aria-components';
import { api } from '../api';
import type { SyncState } from '../state';
import { Caution, ComputerList, Group, Link, Push, Row, Toggle } from '../ui';

/** Where an app comes from, as a short tag and what it means. */
function source(from: string | undefined): [string, string] {
  if (from === 'omarchy') return ['Omarchy', "From Omarchy's own repository."];
  if (from === 'arch') return ['Arch', "From Arch Linux's official repositories."];
  if (from === 'aur')
    return [
      'AUR',
      "From the Arch User Repository: made by the community, used only when Omarchy and Arch don't have it.",
    ];
  if (from === 'elsewhere') return ['No repository', 'Installs with the steps you add.'];
  return from
    ? [
        from,
        `From the ${from} repository, which each computer needs added (the Sync page offers it).`,
      ]
    : ['', ''];
}

/** Install steps in a few words: the only one, or how many and the first. */
const steps = (command: string) => {
  const lines = command.split('\n').filter((l) => l.trim());
  return lines.length === 1 ? `“${lines[0]}”` : `${lines.length} steps, starting “${lines[0]}”`;
};

/**
 * How to install an app that isn't in Arch's repos or the AUR: its steps, one command per
 * line, run in an empty folder in a terminal window. It starts empty; the person types them.
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
        <b>Only enter steps from a source you fully trust.</b> They run on every computer that
        installs {app}, with your access: they can read, change or delete anything you can. Never
        paste commands from someone you don't know.
      </Caution>
      <TextField className="field labelled" value={command} onChange={setCommand} isRequired>
        <Label>Install steps</Label>
        <TextArea rows={5} spellCheck={false} />
        <Text slot="description">
          One command per line, such as downloading the app with git clone, building it and running
          its install script. They run in order in an empty folder, in a terminal window you can
          see, and stop at the first step that fails.
        </Text>
      </TextField>
      <div className="actions">
        <Link onPress={onDone}>Cancel</Link>
        <Push disabled={!s.idle || !command.trim()} onPress={save}>
          Save Install Steps
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
  // Past three computers a column each gets too narrow: each app lists its computers instead.
  const many = computers.length > 3;
  const apps = [...new Set([...data.apps, ...ov.apps.mine])].toSorted();
  // Sync All leaves out apps that can't sync yet (no install steps).
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
        footer="Switch an app on for a computer to have it there; it installs after you approve it. Switching it off never uninstalls it. Apps come from Omarchy's repository first, then Arch's, and the AUR only when neither has them."
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
            {computers.length > 1 && !many && (
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
              // Not in Arch's repos or the AUR: it syncs only with its install steps.
              const outside = ov.apps.outside.includes(app) || app in (data.outside ?? {});
              const script = data.outside?.[app] ?? null;
              const [tag, meaning] = source(ov.apps.source[app]);
              return (
                <div className={`row matrix ${many ? 'many' : ''}`} style={columns} key={app}>
                  <div className="text">
                    <div>
                      {app}
                      {tag && (
                        <span className={`source ${ov.apps.source[app]}`} title={meaning}>
                          {tag}
                        </span>
                      )}
                    </div>
                    {missingHere ? (
                      <div className="detail warn">
                        {script
                          ? 'Installs here with its install steps'
                          : 'Installs here after you approve it'}
                      </div>
                    ) : (
                      <div className="detail">{synced ? about : 'Only on this computer'}</div>
                    )}
                    {outside && (
                      <div className="detail">
                        {script
                          ? `Not in Arch's repos or the AUR. Installs with ${steps(script.command)}.`
                          : "Not in Arch's repos or the AUR. Add its install steps to sync it."}
                      </div>
                    )}
                    {outside && editing !== app && (
                      <div className="inline">
                        <Link disabled={!s.idle} onPress={() => setEditing(app)}>
                          {script ? 'Edit Install Steps…' : 'Add Install Steps…'}
                        </Link>
                        {script && (
                          <Link
                            disabled={!s.idle}
                            onPress={() => s.edit(() => api.removeAppScript(app))}
                          >
                            Remove Install Steps
                          </Link>
                        )}
                      </div>
                    )}
                    {editing === app && (
                      <ScriptForm s={s} app={app} onDone={() => setEditing(null)} />
                    )}
                  </div>
                  {many ? (
                    <ComputerList
                      subject={app}
                      computers={computers.map((c) => ({
                        id: c,
                        name: name(c),
                        on: s.wants(c, app),
                      }))}
                      disabled={!s.idle || (outside && !script)}
                      onChange={(c, on) => s.edit(() => api.setApp(c, app, on))}
                    />
                  ) : (
                    computers.map((c) => (
                      <div className="cell" key={c}>
                        <span className="cell-label">{column(c)}</span>
                        <Toggle
                          on={s.wants(c, app)}
                          label={`${app} synced on ${name(c)}`}
                          disabled={!s.idle || (outside && !script)}
                          onChange={(on) => s.edit(() => api.setApp(c, app, on))}
                        />
                      </div>
                    ))
                  )}
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
