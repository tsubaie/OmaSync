import { useState } from 'react';
import { Package } from 'lucide-react';
import { api, type MissingRepo } from '../api';
import type { SyncState } from '../state';
import { Caution, Group, Link, Push, Row, Tile } from '../ui';

/**
 * The commands that add a repository by hand, in a terminal: the same steps Add Repository
 * takes (its settings file, an Include in pacman.conf, its key trusted, lists refreshed).
 */
function repoCommands(r: MissingRepo) {
  const file = `/etc/pacman.d/omarchy-sync-${r.name}.conf`;
  const conf = [
    `[${r.name}]`,
    ...(r.repository.sig_level.length ? [`SigLevel = ${r.repository.sig_level.join(' ')}`] : []),
    ...r.repository.servers.map((s) => `Server = ${s}`),
  ];
  const key = r.repository.key;
  return [
    `sudo tee ${file} > /dev/null <<'EOF'`,
    ...conf,
    'EOF',
    `grep -qxF 'Include = ${file}' /etc/pacman.conf || echo 'Include = ${file}' | sudo tee -a /etc/pacman.conf`,
    ...(key
      ? [
          `cat > /tmp/${r.name}.key <<'EOF'`,
          key.armored.trim(),
          'EOF',
          `sudo pacman-key --add /tmp/${r.name}.key`,
          `sudo pacman-key --lsign-key ${key.fingerprint}`,
        ]
      : []),
    'sudo pacman -Sy',
  ].join('\n');
}

/** The fingerprint in groups of four, as vendors print it, to compare at a glance. */
const groups = (fingerprint: string) => fingerprint.match(/.{1,4}/g)?.join(' ') ?? fingerprint;

/**
 * Extra repositories this computer needs before some synced apps can install (they never come
 * from the AUR instead). Adding one changes what this computer trusts, so it always asks, with
 * the repository's address and key shown, and asks for the password.
 */
function ReposToAdd({ s }: { s: SyncState }) {
  const [confirm, setConfirm] = useState<string | null>(null);
  const [shown, setShown] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  return (
    <Group
      title="Repositories to Add"
      footer="These apps come from their makers' own repositories, which your other computers use. They install here once the repository is added."
    >
      {s.overview!.missing_repos.map((r) => (
        <Row
          key={r.name}
          lead={<Tile icon={Package} kind="apps" size={24} />}
          title={r.name}
          detail={`Needed for ${r.apps.join(', ')}. From ${r.repository.servers[0]}`}
          end={
            confirm === r.name ? (
              <div className="actions">
                <Link onPress={() => setConfirm(null)}>Cancel</Link>
                <Push
                  onPress={() => {
                    setConfirm(null);
                    s.act(
                      `Adding the ${r.name} repository`,
                      () => api.addRepository(r.name),
                      () => s.refresh(),
                    );
                  }}
                >
                  Add It
                </Push>
              </div>
            ) : (
              <Push disabled={!s.idle} onPress={() => setConfirm(r.name)}>
                Add Repository…
              </Push>
            )
          }
        >
          <div className="inline">
            <Link onPress={() => setShown(shown === r.name ? null : r.name)}>
              {shown === r.name ? 'Hide Commands' : 'Show Commands'}
            </Link>
          </div>
          {shown === r.name && (
            <div className="commands">
              <p className="detail">
                Or add it yourself: run these in a terminal. They do the same as Add Repository.
              </p>
              <pre>
                <code>{repoCommands(r)}</code>
              </pre>
              <Push
                onPress={() =>
                  navigator.clipboard.writeText(repoCommands(r)).then(() => setCopied(true), s.fail)
                }
              >
                {copied ? 'Copied' : 'Copy Commands'}
              </Push>
            </div>
          )}
          {confirm === r.name && (
            <Caution>
              <b>Add it only if you trust {r.repository.key?.owner || r.name}.</b> From now on this
              computer installs and updates packages from it, as your other computers do.
              {r.repository.key ? (
                <>
                  {' '}
                  Its key's fingerprint, to check against the maker's website:{' '}
                  <code>{groups(r.repository.key.fingerprint)}</code>
                </>
              ) : (
                ' It has no signing key, so its packages can’t be checked.'
              )}
            </Caution>
          )}
        </Row>
      ))}
    </Group>
  );
}

/** Extra repositories (beyond Arch's and Omarchy's) that some synced apps come from. */
export function ReposPane({ s }: { s: SyncState }) {
  const ov = s.overview!;
  return (
    <>
      <p className="lede">
        Some apps come from their makers' own repositories instead of Arch or the AUR. Each computer
        needs the repository before those apps can install there.
      </p>
      {ov.missing_repos.length > 0 && <ReposToAdd s={s} />}
      <Group title="On This Computer">
        {ov.repos_here.length === 0 ? (
          <Row title="No extra repositories." />
        ) : (
          ov.repos_here.map((r) => (
            <Row
              key={r.name}
              lead={<Tile icon={Package} kind="apps" size={24} />}
              title={r.name}
              detail={[
                r.apps.length > 0 && `For ${r.apps.join(', ')}.`,
                `From ${r.repository.servers[0]}`,
              ]
                .filter(Boolean)
                .join(' ')}
            />
          ))
        )}
      </Group>
    </>
  );
}
