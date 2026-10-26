import { useState } from 'react';
import { Search } from 'lucide-react';
import { Input, Label, SearchField, Text, TextArea, TextField } from 'react-aria-components';
import { api } from '../api';
import type { SyncState } from '../state';
import { Caution, Group, Link, Push, Row, Toggle } from '../ui';

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
  if (from === 'distro') return ['System', "From this computer's Linux distribution."];
  return from
    ? [
        from,
        `From the ${from} repository, which each computer needs added (the Sync page offers it).`,
      ]
    : ['', ''];
}

/** Install steps in a few words: the only one, or how many and the first. */
/** A website's host, or null when the package's URL isn't one. */
function hostOf(url?: string): string | null {
  try {
    return url ? new URL(url).host : null;
  } catch {
    return null;
  }
}

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

/**
 * One app: its one Sync switch (every computer), and like Configurations a link to leave only
 * this computer out. One that isn't installed here has Install beside its switch.
 */
function AppRow({
  s,
  app,
  editing,
  setEditing,
}: {
  s: SyncState;
  app: string;
  editing: boolean;
  setEditing: (app: string | null) => void;
}) {
  const { overview: ov, data, host } = s;
  if (!ov || !data) return null;
  const many = ov.computers.length > 1;
  const synced = data.apps.includes(app);
  const here = s.wants(host, app);
  const installed = ov.apps.installed.includes(app);
  const about = ov.apps.about[app];
  // Not in Arch's repos or the AUR: it syncs only with its install steps.
  const outside = ov.apps.outside.includes(app) || app in (data.outside ?? {});
  const script = data.outside?.[app] ?? null;
  const locked = outside && !script;
  const from = ov.apps.source[app];
  const [tag, meaning] = source(from);
  // Where its link goes: Arch's or the AUR's package page, which say everything about it; an
  // app from Omarchy's repository (no pages) links to its own website.
  const site = hostOf(ov.apps.website[app]);
  const page =
    from === 'arch'
      ? (['arch', 'archlinux.org'] as const)
      : from === 'aur'
        ? (['aur', 'the AUR'] as const)
        : from === 'omarchy' && site
          ? (['website', site] as const)
          : null;
  // Needs an extra repository this computer doesn't have yet: it can't install here until
  // that's added (it never comes from the AUR instead).
  const repo = ov.missing_repos.find((r) => r.apps.includes(app))?.name;
  const toggle = (
    <Toggle
      on={synced}
      label={many ? `Sync ${app} (all computers)` : `Sync ${app}`}
      disabled={!s.idle || locked || !!repo}
      onChange={(on) => s.edit(() => api.setApp(app, on))}
    />
  );
  return (
    <Row
      title={
        <>
          {app}
          {tag && (
            <span className={`source ${from}`} title={meaning}>
              {tag}
            </span>
          )}
        </>
      }
      detail={synced ? about : 'Only on this computer'}
      end={
        !installed && here && !repo ? (
          <div className="inline">
            {/* Install steps run only after the Sync page's warning and a second yes. */}
            <Push
              disabled={!s.idle}
              onPress={() =>
                script
                  ? s.setTab('sync')
                  : s.act(
                      `Installing ${app}`,
                      () => api.installApps([app]),
                      (snapshot) => {
                        s.changed(`Installed ${app}.`, snapshot);
                        return s.refresh();
                      },
                    )
              }
            >
              {script ? 'Install…' : 'Install'}
            </Push>
            {toggle}
          </div>
        ) : (
          toggle
        )
      }
    >
      {repo && (
        <div className="detail warn">
          Needs the {repo} repository on this computer first (see Sync).
        </div>
      )}
      {outside && (
        <div className="detail">
          {script
            ? `Not in Arch's repos or the AUR. Installs with ${steps(script.command)}.`
            : "Not in Arch's repos or the AUR. Add its install steps to sync it."}
        </div>
      )}
      {synced && !locked && (many || !here) && (
        <div className="inline">
          {here ? (
            <span className="detail">On for all your computers, this one included.</span>
          ) : (
            <span className="detail warn">
              Not synced on this computer: it isn't offered here while your other computers stay in
              step.
            </span>
          )}
          <Link disabled={!s.idle} onPress={() => s.edit(() => api.appHere(app, !here))}>
            {here ? "Don't Sync on This Computer" : 'Sync on This Computer'}
          </Link>
        </div>
      )}
      {page && (
        <div className="inline">
          <Link
            out
            label={`Open ${page[1]} (${app})`}
            onPress={() => api.openPage(page[0], app).catch(s.fail)}
          >
            Open {page[1]}
          </Link>
        </div>
      )}
      {outside && !editing && (
        <div className="inline">
          <Link disabled={!s.idle} onPress={() => setEditing(app)}>
            {script ? 'Edit Install Steps…' : 'Add Install Steps…'}
          </Link>
          {script && (
            <Link disabled={!s.idle} onPress={() => s.edit(() => api.removeAppScript(app))}>
              Remove Install Steps
            </Link>
          )}
        </div>
      )}
      {editing && <ScriptForm s={s} app={app} onDone={() => setEditing(null)} />}
    </Row>
  );
}

/** Installed apps first, then those your other computers sync that this one doesn't have. */
export function AppsPane({ s }: { s: SyncState }) {
  const [query, setQuery] = useState('');
  const [editing, setEditing] = useState<string | null>(null);
  const { overview: ov, data } = s;
  if (!ov || !data) return null;
  const apps = [...new Set([...data.apps, ...ov.apps.mine])].toSorted();
  const q = query.trim().toLowerCase();
  const shown = apps.filter(
    (app) => !q || app.includes(q) || (ov.apps.about[app] ?? '').toLowerCase().includes(q),
  );
  const installedShown = shown.filter((app) => ov.apps.installed.includes(app));
  const uninstalledShown = shown.filter((app) => !ov.apps.installed.includes(app));
  const row = (app: string) => (
    <AppRow key={app} s={s} app={app} editing={editing === app} setEditing={setEditing} />
  );
  return (
    <>
      <Group
        title="Your Apps"
        footer="A switch syncs an app to all your computers, where it installs after you approve it, or stops it everywhere; Don't Sync on This Computer leaves only this computer out. Switching it off never uninstalls it. Apps come from Omarchy's repository first, then Arch's, and the AUR only when neither has them."
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
        ) : installedShown.length === 0 ? (
          <Row title="None of these are installed on this computer." />
        ) : (
          installedShown.map(row)
        )}
      </Group>
      {uninstalledShown.length > 0 && (
        <Group
          title="Not Installed on This Computer"
          footer="Synced from your other computers. Install one here, or leave this computer out."
        >
          {uninstalledShown.map(row)}
        </Group>
      )}
    </>
  );
}
