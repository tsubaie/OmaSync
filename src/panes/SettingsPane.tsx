import { useState } from 'react';
import { Monitor } from 'lucide-react';
import { Input, TextField } from 'react-aria-components';
import { api } from '../api';
import { ago } from '../format';
import type { SyncState } from '../state';
import { Group, Link, Push, Row, Tile, Toggle } from '../ui';

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
          detail="Every 15 minutes, even when this window is closed. You're notified only when a choice is needed."
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

      <Group title="Keyboard Shortcuts">
        {(
          [
            ['Sync now', ['Ctrl', 'R']],
            ['Go to a section', ['Ctrl', '1–7']],
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
