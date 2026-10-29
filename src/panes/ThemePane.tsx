import { useEffect, useEffectEvent, useState } from 'react';
import {
  FolderClosed,
  Layers,
  Mic,
  Palette,
  PanelBottom,
  PanelTop,
  Rows3,
  Search,
  Volume2,
  type LucideIcon,
} from 'lucide-react';
import { api, type MacosTheme } from '../api';
import { shortList } from '../format';
import type { SyncState } from '../state';
import { Group, Link, Progress, Push, Row, Tile, Toggle } from '../ui';
import desktop from '../assets/macos-theme/desktop.jpg';
import dock from '../assets/macos-theme/dock-menu.jpg';
import switcher from '../assets/macos-theme/app-switcher.jpg';
import hud from '../assets/macos-theme/dictation-hud.jpg';

type Piece = { id: string; name: string; about: string; icon: LucideIcon; shot?: string };

// The pieces of github.com/tsubaie/omarchy-macos-theme, by the names its install.sh uses.
const LOOK: Piece[] = [
  {
    id: 'theme',
    name: 'Theme',
    about: 'Apple’s colors, frosted glass, title bars with traffic lights, gaps and motion.',
    icon: Palette,
  },
  {
    id: 'rows',
    name: 'Striped Rows in Files',
    about: 'Finder-style rows in list view, while the theme is on.',
    icon: Rows3,
  },
  {
    id: 'icons',
    name: 'macOS Icons',
    about: 'The MacTahoe icon theme, downloaded from GitHub (about 100 MB).',
    icon: FolderClosed,
  },
];
const DESKTOP: Piece[] = [
  {
    id: 'dock',
    name: 'Dock',
    about: 'Magnification, running dots, a Downloads stack and Trash.',
    icon: PanelBottom,
    shot: dock,
  },
  {
    id: 'menubar',
    name: 'Menu Bar',
    about: 'The app’s name in bold, Control Center by the clock, and Inter.',
    icon: PanelTop,
  },
  {
    id: 'switcher',
    name: 'App Switcher',
    about: 'Alt+Tab for apps and Super+Tab for workspaces.',
    icon: Layers,
    shot: switcher,
  },
  {
    id: 'wide-menu',
    name: 'Spotlight-Like Menu',
    about: 'Omarchy’s menu and launcher, wider, with a search line.',
    icon: Search,
  },
  {
    id: 'osd',
    name: 'Volume Capsule',
    about: 'Volume and brightness as a frosted pill.',
    icon: Volume2,
  },
  {
    id: 'dictation-hud',
    name: 'Dictation Pill',
    about: 'A recording pill for Voxtype dictation.',
    icon: Mic,
    shot: hud,
  },
];
const NAMES = Object.fromEntries([...LOOK, ...DESKTOP].map((p) => [p.id, p.name]));

/**
 * The macOS theme for Omarchy: a switch per piece, applied together with Apply, which runs the
 * theme's own install.sh and shows its steps.
 */
export function ThemePane({ s }: { s: SyncState }) {
  // Not checked yet (undefined), not downloaded (null), or what is installed.
  const [theme, setTheme] = useState<MacosTheme | null | undefined>(undefined);
  /** Switches flipped and not applied yet. */
  const [draft, setDraft] = useState<Record<string, boolean>>({});
  const [working, setWorking] = useState(false);

  const load = () => api.macosTheme().then(setTheme, s.fail);
  const onOpen = useEffectEvent(() => {
    load();
  });
  useEffect(() => onOpen(), []);

  if (theme === undefined) return null;
  const installed = (id: string) => !!theme?.features[id];
  const on = (id: string) => draft[id] ?? installed(id);
  const add = Object.keys(NAMES).filter((id) => on(id) && !installed(id));
  const remove = Object.keys(NAMES).filter((id) => !on(id) && installed(id));
  const changed = add.length + remove.length > 0;

  const job = (label: string, work: (step: (line: string) => void) => Promise<string>) => {
    setWorking(true);
    return s
      .act(label, work, async (log) => {
        setDraft({});
        s.setNotice({
          good: true,
          text: log.trim().split('\n').at(-1)?.replace(/^==> /, '') ?? '',
          details: log,
        });
        await load();
      })
      .finally(() => {
        setWorking(false);
        load();
      });
  };
  const apply = () => job('Applying', (step) => api.applyMacosTheme(add, remove, step));
  const update = () => job('Updating the macOS theme', (step) => api.updateMacosTheme(step));

  const status =
    theme === null
      ? 'Not installed. Choose the pieces you want, then press Install.'
      : theme.active
        ? 'Your current theme.'
        : installed('theme')
          ? 'Installed. Choose it with Omarchy’s theme picker to see it.'
          : 'Choose the pieces you want, then press Apply.';

  const summary = [
    add.length
      ? `Installs ${shortList(
          add.map((id) => NAMES[id]),
          9,
        )}.`
      : '',
    remove.length
      ? `Removes ${shortList(
          remove.map((id) => NAMES[id]),
          9,
        )}.`
      : '',
  ]
    .filter(Boolean)
    .join(' ');

  const row = (p: Piece) => {
    // The theme can only go once another theme is current, so Omarchy is never left without one.
    const locked = p.id === 'theme' && !!theme?.active && on('theme');
    return (
      <Row
        key={p.id}
        lead={<Tile icon={p.icon} kind={on(p.id) ? 'accent' : 'plain'} size={26} />}
        title={p.name}
        detail={locked ? 'To remove it, switch to another theme first.' : p.about}
        end={
          <div className="inline">
            {p.shot && <img className="shot-thumb" src={p.shot} alt="" />}
            <Toggle
              on={on(p.id)}
              label={p.name}
              disabled={!s.idle || locked}
              onChange={(v) => setDraft((d) => ({ ...d, [p.id]: v }))}
            />
          </div>
        }
      />
    );
  };

  return (
    <>
      <div className="box theme-hero">
        <img
          className="shot"
          src={desktop}
          alt="A desktop with the macOS theme: a frosted menu bar, a dock and glass windows"
        />
        <div className="about">
          <div className="text">
            <h2>macOS Theme</h2>
            <p className="detail">{status}</p>
          </div>
          {theme !== null && (
            <Push
              disabled={!s.idle || changed}
              onPress={update}
              tip="Get the latest version and install your pieces again"
            >
              Update
            </Push>
          )}
        </div>
      </div>

      <Group title="Look">{LOOK.map(row)}</Group>
      <Group
        title="Desktop"
        footer="Each piece replaces or adds to Omarchy’s own. Removing one brings Omarchy’s back."
      >
        {DESKTOP.map(row)}
      </Group>

      {working && (
        <div className="box row">
          <div className="text">
            <Progress steps={s.steps} plan={s.steps} percent={null} />
          </div>
        </div>
      )}

      {/* Floats at the foot of the pane while there is something to apply. */}
      {changed && (
        <div className="apply-bar">
          <p className="detail">{summary}</p>
          <Link disabled={!s.idle} onPress={() => setDraft({})}>
            Revert
          </Link>
          <Push prominent disabled={!s.idle} onPress={apply}>
            {theme === null ? 'Install' : 'Apply'}
          </Push>
        </div>
      )}
    </>
  );
}
