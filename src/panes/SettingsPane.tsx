import { useState } from 'react';
import { CircleCheck, Download, Monitor } from 'lucide-react';
import { Input, TextField } from 'react-aria-components';
import { api, type Update } from '../api';
import { ago } from '../format';
import type { SyncState } from '../state';
import { Group, Link, Progress, Push, Row, Tile, Toggle } from '../ui';

/** This computer's name: saved when you press Return or leave the field. */
function NameField({ s }: { s: SyncState }) {
  const current = s.nameOf(s.host);
  const [draft, setDraft] = useState(current);
  const save = () => {
    const name = draft.trim();
    if (name && name !== current) s.edit(() => api.renameComputer(name));
    else setDraft(current);
  };
  return (
    <TextField
      className="field"
      aria-label="This computer's name"
      value={draft}
      onChange={setDraft}
      onBlur={save}
      onKeyDown={(e) => e.key === 'Enter' && save()}
      maxLength={40}
      isDisabled={!s.idle}
    >
      <Input />
    </TextField>
  );
}

/**
 * Omarchy Sync's own version, and a newer release when there is one: the update is downloaded,
 * checked, installed after the password, and the app restarts once any running sync is done.
 */
function Updates({ s }: { s: SyncState }) {
  // Not checked yet (undefined), up to date (null), or the newer release.
  const [found, setFound] = useState<Update | null | undefined>(undefined);
  const [confirm, setConfirm] = useState(false);
  // Only an update's own progress shows here, not a sync's: both update plans end this way.
  const updating = s.plan.includes('Restarting Omarchy Sync');
  const title = `Omarchy Sync ${s.version}`;
  if (s.installedHow === 'Source') {
    return (
      <Group
        title="Updates"
        footer="This copy was built from source. To update, pull the latest code and run the build steps in the README again."
      >
        <Row title={title} detail="Built from source" />
      </Group>
    );
  }
  // The answer sits in the row itself, marked the way the Sync page marks good news and updates.
  const detail =
    found === undefined ? (
      s.installedHow === 'Aur' ? (
        'Installed from the AUR'
      ) : (
        'Installed from the release package'
      )
    ) : found === null ? (
      <span className="good">
        <CircleCheck size={13} strokeWidth={2.25} aria-hidden="true" />
        You have the latest version.
      </span>
    ) : (
      <span className="accent">
        <Download size={13} strokeWidth={2.25} aria-hidden="true" />
        {found.version} is available.
      </span>
    );
  const checking = s.busy === 'Checking for updates';
  const check = () =>
    s.act(
      'Checking for updates',
      () => api.checkUpdate(),
      (update) => {
        setFound(update);
        // The row's detail says it too, but a check that finds nothing needs an unmistakable answer.
        if (!update) s.flash('You have the latest version');
      },
    );
  return (
    <Group
      title="Updates"
      footer="Updates come from Omarchy Sync's releases on GitHub. Before one is installed, it's checked against the release's checksums and GitHub's record of which workflow built it, and Omarchy asks for your password."
    >
      <Row
        title={title}
        detail={detail}
        end={
          found ? (
            confirm ? (
              <div className="actions">
                <Link onPress={() => setConfirm(false)}>Cancel</Link>
                <Push
                  prominent
                  disabled={!s.idle}
                  onPress={() => {
                    setConfirm(false);
                    s.act(`Updating to ${found.version}`, (onStep) =>
                      api.installUpdate(found.version, onStep),
                    );
                  }}
                >
                  Update
                </Push>
              </div>
            ) : (
              <div className="actions">
                <Link
                  disabled={!s.idle}
                  onPress={() => api.openRelease(found.version).catch(s.fail)}
                >
                  What's New
                </Link>
                <Push disabled={!s.idle} onPress={() => setConfirm(true)}>
                  {updating ? 'Updating…' : `Update to ${found.version}…`}
                </Push>
              </div>
            )
          ) : (
            <Push disabled={!s.idle} onPress={check}>
              {checking ? 'Checking…' : found === null ? 'Check Again' : 'Check for Updates'}
            </Push>
          )
        }
      >
        {confirm && found && (
          <p className="detail">
            Downloads Omarchy Sync {found.version} from GitHub, checks that its release workflow
            built it, installs it after your password, then restarts Omarchy Sync. A sync that's
            running finishes first.
          </p>
        )}
        {updating && <Progress steps={s.steps} plan={s.plan} percent={s.percent} />}
      </Row>
    </Group>
  );
}

export function SettingsPane({ s }: { s: SyncState }) {
  const [confirm, setConfirm] = useState<string | null>(null);
  // The bar switch's new position while the bar is being changed.
  const [barOn, setBarOn] = useState<boolean | null>(null);
  const ov = s.overview;
  if (!ov || !s.data) return null;
  const space = s.syncSpace;
  const others = ov.computers.filter(([id]) => id !== s.host);

  const disconnect = () =>
    s.act(
      'Disconnecting',
      () => api.disconnect(),
      (moved) => {
        s.setNotice({
          good: true,
          text: `This computer stopped syncing. Its sync folder was moved to ${moved}.`,
        });
        return s.probe();
      },
    );

  return (
    <>
      <Group
        title="This Computer"
        footer="Your other computers see this name. It doesn't change the computer's hostname."
      >
        <Row title="Name" end={<NameField key={s.nameOf(s.host)} s={s} />} />
      </Group>

      <Group
        title="Sync Space"
        footer="Everything syncs through this private repository. Only you can see it."
      >
        <Row
          title={space ? space.replace('https://', '') : 'Not connected'}
          detail="On GitHub"
          end={
            space && <Push onPress={() => api.openSyncSpace().catch(s.fail)}>Open in Browser</Push>
          }
        />
      </Group>

      <Group
        title="Other Computers"
        footer="Removing only takes a computer off this list, for one you no longer use. If it syncs again, it comes back."
      >
        {others.length === 0 && (
          <Row title="No other computers yet." detail="Add one from the Sync pane." />
        )}
        {others.map(([id, when]) => (
          <Row
            key={id}
            lead={<Tile icon={Monitor} kind="plain" size={26} />}
            title={s.nameOf(id)}
            detail={when ? `Last shared changes ${ago(when, s.now)}` : "Hasn't shared anything yet"}
            end={
              confirm === id ? (
                <div className="actions">
                  <Link onPress={() => setConfirm(null)}>Cancel</Link>
                  <Push
                    onPress={() => {
                      setConfirm(null);
                      s.edit(() => api.forgetComputer(id));
                    }}
                  >
                    Remove
                  </Push>
                </div>
              ) : (
                <Link disabled={!s.idle} onPress={() => setConfirm(id)}>
                  Remove…
                </Link>
              )
            }
          />
        ))}
      </Group>

      <Group title="Automatic Syncing">
        <Row
          title="Sync automatically"
          detail="Once a day at midnight, even when this window is closed; a computer that was off then catches up when it starts. You're notified only when a choice is needed."
          end={
            <Toggle
              on={!s.me.manual}
              label="Sync automatically"
              disabled={!s.idle}
              onChange={(on) => s.edit(() => api.setAuto(on))}
            />
          }
        />
        <Row
          title="Add synced apps and plugins without asking"
          detail="When off, new apps and plugins wait on the Sync pane for you to approve them."
          end={
            <Toggle
              on={!!s.me.install_without_asking}
              label="Add synced apps and plugins without asking"
              disabled={!s.idle}
              onChange={(on) => s.edit(() => api.setInstallAuto(on))}
            />
          }
        />
      </Group>

      <Group
        title="Omarchy Bar"
        footer="Omarchy Sync's icon on your bar: spinning in green while syncing, staying green for a few seconds when a sync finishes well, red when syncing fails, and yellow when something waits for you. Click it to open Omarchy Sync."
      >
        <Row
          title="Show in the bar"
          detail="On by default. Switching it off removes the indicator from this computer's bar."
          end={
            <Toggle
              on={barOn ?? s.barIndicator}
              label="Show in the bar"
              disabled={!s.idle}
              onChange={(on) => {
                // Flips at once; the bar catches up in the background.
                setBarOn(on);
                s.act(
                  on ? 'Adding to the bar' : 'Removing from the bar',
                  () => api.setBarIndicator(on),
                  async () => {
                    await s.refresh();
                    setBarOn(null);
                  },
                );
              }}
            />
          }
        />
      </Group>

      <Updates s={s} />

      <Group title="Keyboard Shortcuts">
        {(
          [
            ['Sync now', ['Ctrl', 'R']],
            ['Go to a section', ['Ctrl', '1–8']],
            ['Move between controls', ['Tab']],
            ['Cancel a confirmation', ['Esc']],
            ['Quit', ['Ctrl', 'Q']],
          ] as const
        ).map(([action, keys]) => (
          <Row
            key={action}
            title={action}
            end={
              <span className="inline">
                {keys.map((k) => (
                  <kbd key={k}>{k}</kbd>
                ))}
              </span>
            }
          />
        ))}
      </Group>

      <Group
        title="Stop Syncing This Computer"
        footer="Your settings here stay exactly as they are, and your other computers keep syncing. You can set this computer up again any time."
      >
        <Row
          title="Disconnect this computer"
          detail="Turns off automatic syncing and moves the local sync folder aside."
          end={
            confirm === 'disconnect' ? (
              <div className="actions">
                <Link onPress={() => setConfirm(null)}>Cancel</Link>
                <Push
                  onPress={() => {
                    setConfirm(null);
                    disconnect();
                  }}
                >
                  Disconnect
                </Push>
              </div>
            ) : (
              <Push disabled={!s.idle} onPress={() => setConfirm('disconnect')}>
                Disconnect…
              </Push>
            )
          }
        />
      </Group>
    </>
  );
}
