import { useEffect, useEffectEvent, useState } from 'react';
import {
  Blend,
  ChevronRight,
  FolderClosed,
  Layers,
  Mic,
  Palette,
  PanelBottom,
  PanelTop,
  Rows3,
  Search,
  Volume2,
  ZoomIn,
  type LucideIcon,
} from 'lucide-react';
import { Button } from 'react-aria-components';
import { api, type MacosTheme } from '../api';
import { shortList } from '../format';
import type { SyncState, Tab } from '../state';
import { Group, Link, Progress, Push, Row, Slider, Tile, Toggle } from '../ui';
import desktop from '../assets/macos-theme/desktop.jpg';
import titleBar from '../assets/macos-theme/title-bar.png';
import dock from '../assets/macos-theme/dock.webp';
import switcher from '../assets/macos-theme/app-switcher.webp';
import hud from '../assets/macos-theme/dictation-hud.png';

type Piece = { name: string; about: string; icon: LucideIcon };

// The pieces of github.com/tsubaie/omarchy-macos-theme, by the names its install.sh uses.
const PIECES: Record<string, Piece> = {
  theme: {
    name: 'Theme',
    about: 'Apple’s colors, frosted glass, title bars with traffic lights, gaps and motion.',
    icon: Palette,
  },
  rows: {
    name: 'Striped Rows in Files',
    about: 'Finder-style rows in list view, while the theme is on.',
    icon: Rows3,
  },
  icons: {
    name: 'macOS Icons',
    about: 'The MacTahoe icon theme, downloaded from GitHub (about 100 MB).',
    icon: FolderClosed,
  },
  dock: {
    name: 'Dock',
    about: 'Magnification, running dots, a Downloads stack and Trash.',
    icon: PanelBottom,
  },
  menubar: {
    name: 'Menu Bar',
    about: 'The app’s name in bold, Control Center by the clock, and Inter.',
    icon: PanelTop,
  },
  switcher: {
    name: 'App Switcher',
    about: 'Alt+Tab for apps and Super+Tab for workspaces.',
    icon: Layers,
  },
  'wide-menu': {
    name: 'Spotlight-Like Menu',
    about: 'Omarchy’s menu and launcher, wider, with a search line.',
    icon: Search,
  },
  osd: { name: 'Volume Capsule', about: 'Volume and brightness as a frosted pill.', icon: Volume2 },
  'dictation-hud': {
    name: 'Dictation Pill',
    about: 'A recording pill for Voxtype dictation.',
    icon: Mic,
  },
};

type Setting = Piece & {
  /** The piece it adjusts, which has to be on. */
  needs: string;
  min: number;
  max: number;
  ends: [string, string];
  /** How the value is read out, and in the summary. */
  format: Intl.NumberFormatOptions;
  /** After the number in the summary, where Intl has no such unit. */
  unit?: string;
};
// The settings install.sh --set takes, with its ranges.
const SETTINGS: Record<string, Setting> = {
  glass: {
    name: 'Window Glass',
    about: 'How solid windows are. Terminals, Files, browsers and video keep their own.',
    icon: Blend,
    needs: 'theme',
    min: 0.5,
    max: 1,
    ends: ['Clear', 'Solid'],
    format: { style: 'percent' },
  },
  magnification: {
    name: 'Dock Magnification',
    about: 'How far icons grow under the pointer.',
    icon: ZoomIn,
    needs: 'dock',
    min: 1,
    max: 2,
    ends: ['Off', 'Large'],
    format: { maximumFractionDigits: 2 },
    unit: '×',
  },
};

type Page = {
  id: Tab;
  name: string;
  icon: LucideIcon;
  pieces: string[];
  settings: string[];
  shot?: string;
};
/** The theme's pages, each listed under macOS Theme in the sidebar. */
export const THEME_PAGES: Page[] = [
  {
    id: 'theme-look',
    name: 'Appearance',
    icon: Palette,
    pieces: ['theme', 'rows', 'icons'],
    settings: ['glass'],
    shot: titleBar,
  },
  {
    id: 'theme-dock',
    name: 'Dock',
    icon: PanelBottom,
    pieces: ['dock'],
    settings: ['magnification'],
    shot: dock,
  },
  { id: 'theme-menubar', name: 'Menu Bar', icon: PanelTop, pieces: ['menubar'], settings: [] },
  {
    id: 'theme-switcher',
    name: 'App Switcher',
    icon: Layers,
    pieces: ['switcher'],
    settings: [],
    shot: switcher,
  },
  { id: 'theme-menu', name: 'Spotlight Menu', icon: Search, pieces: ['wide-menu'], settings: [] },
  { id: 'theme-osd', name: 'Volume Capsule', icon: Volume2, pieces: ['osd'], settings: [] },
  {
    id: 'theme-hud',
    name: 'Dictation Pill',
    icon: Mic,
    pieces: ['dictation-hud'],
    settings: [],
    shot: hud,
  },
];

/**
 * The theme's state, shared by its pages so a change waits for Apply while you move between
 * them: what is installed, the switches flipped and the sliders moved.
 */
export function useMacosTheme(s: SyncState) {
  // Not checked yet (undefined), not downloaded (null), or what is installed.
  const [theme, setTheme] = useState<MacosTheme | null | undefined>(undefined);
  const [draft, setDraft] = useState<Record<string, boolean>>({});
  const [tweaks, setTweaks] = useState<Record<string, number>>({});
  const [working, setWorking] = useState(false);

  const load = () => api.macosTheme().then(setTheme, s.fail);
  // Checked the first time one of its pages opens.
  const shown = s.tab.startsWith('theme');
  const onShow = useEffectEvent(() => {
    if (theme === undefined) load();
  });
  useEffect(() => {
    if (shown) onShow();
  }, [shown]);

  const installed = (id: string) => !!theme?.features[id];
  const on = (id: string) => draft[id] ?? installed(id);
  const value = (id: string) => tweaks[id] ?? theme?.settings?.[id];
  const add = Object.keys(PIECES).filter((id) => on(id) && !installed(id));
  const remove = Object.keys(PIECES).filter((id) => !on(id) && installed(id));
  const set = Object.keys(SETTINGS)
    .filter((id) => value(id) !== theme?.settings?.[id])
    .map((id) => [id, value(id)!] as [string, number]);
  const revert = () => {
    setDraft({});
    setTweaks({});
  };

  const job = (label: string, work: (step: (line: string) => void) => Promise<string>) => {
    setWorking(true);
    return s
      .act(label, work, async (log) => {
        revert();
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

  return {
    theme,
    working,
    installed,
    on,
    value,
    add,
    remove,
    set,
    changed: add.length + remove.length + set.length > 0,
    flip: (id: string, v: boolean) => setDraft((d) => ({ ...d, [id]: v })),
    // Whole steps, without the float noise of 0.05 sums.
    slide: (id: string, v: number) => setTweaks((d) => ({ ...d, [id]: Math.round(v * 100) / 100 })),
    revert,
    apply: () => job('Applying', (step) => api.applyMacosTheme(add, remove, set, step)),
    update: () => job('Updating the macOS theme', (step) => api.updateMacosTheme(step)),
  };
}
type ThemeState = ReturnType<typeof useMacosTheme>;

/** The overview (`theme`) or one of the theme's pages, with Apply below while something changed. */
export function ThemePane({ s, t, page }: { s: SyncState; t: ThemeState; page: Tab }) {
  if (t.theme === undefined) return null;
  const part = THEME_PAGES.find((p) => p.id === page);
  return (
    <>
      {part ? <PartPage s={s} t={t} part={part} /> : <Overview s={s} t={t} />}
      {t.working && (
        <div className="box row">
          <div className="text">
            <Progress steps={s.steps} plan={s.steps} percent={null} />
          </div>
        </div>
      )}
      {t.changed && <ApplyBar s={s} t={t} />}
    </>
  );
}

function Overview({ s, t }: { s: SyncState; t: ThemeState }) {
  const status =
    t.theme === null
      ? 'Not installed. Choose the parts you want, then press Install.'
      : t.theme?.active
        ? 'Your current theme.'
        : t.installed('theme')
          ? 'Installed. Choose it with Omarchy’s theme picker to see it.'
          : 'Choose the parts you want, then press Apply.';
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
          {t.theme !== null && (
            <Push
              disabled={!s.idle || t.changed}
              onPress={t.update}
              tip="Get the latest version and install your parts again"
            >
              Update
            </Push>
          )}
        </div>
      </div>
      <Group title="Parts">
        {THEME_PAGES.map((p) => {
          const count = p.pieces.filter(t.on).length;
          const state =
            count === p.pieces.length ? 'On' : count ? `${count} of ${p.pieces.length} On` : 'Off';
          return (
            <Button key={p.id} className="row page-link" onPress={() => s.setTab(p.id)}>
              <Tile icon={p.icon} kind={count ? 'accent' : 'plain'} size={26} />
              <span className="text">{p.name}</span>
              <span className="detail">{state}</span>
              <ChevronRight size={14} aria-hidden="true" />
            </Button>
          );
        })}
      </Group>
    </>
  );
}

function PartPage({ s, t, part }: { s: SyncState; t: ThemeState; part: Page }) {
  return (
    <>
      {part.shot && (
        <div className="box part-shot">
          <img src={part.shot} alt={`The ${part.name.toLowerCase()} with the macOS theme`} />
        </div>
      )}
      <Group footer="Each part replaces or adds to Omarchy’s own. Removing one brings Omarchy’s back.">
        {part.pieces.map((id) => {
          const p = PIECES[id];
          // The theme can only go once another theme is current, so Omarchy is never left without one.
          const locked = id === 'theme' && !!t.theme?.active && t.on('theme');
          return (
            <Row
              key={id}
              lead={<Tile icon={p.icon} kind={t.on(id) ? 'accent' : 'plain'} size={26} />}
              title={p.name}
              detail={locked ? 'To remove it, switch to another theme first.' : p.about}
              end={
                <Toggle
                  on={t.on(id)}
                  label={p.name}
                  disabled={!s.idle || locked}
                  onChange={(v) => t.flip(id, v)}
                />
              }
            />
          );
        })}
        {/* A copy of the theme older than its settings has none to show. */}
        {t.theme?.settings &&
          part.settings.map((id) => {
            const x = SETTINGS[id];
            return (
              <Row
                key={id}
                lead={<Tile icon={x.icon} kind={t.on(x.needs) ? 'accent' : 'plain'} size={26} />}
                title={x.name}
                detail={t.on(x.needs) ? x.about : `Turn on ${PIECES[x.needs].name} to adjust it.`}
                end={
                  <Slider
                    value={t.value(id)!}
                    min={x.min}
                    max={x.max}
                    step={0.05}
                    label={x.name}
                    ends={x.ends}
                    format={x.format}
                    disabled={!s.idle || !t.on(x.needs)}
                    onChange={(v) => t.slide(id, v)}
                  />
                }
              />
            );
          })}
      </Group>
    </>
  );
}

const names = (ids: string[]) =>
  shortList(
    ids.map((id) => PIECES[id].name),
    9,
  );

/** Floats at the foot of every theme page while there is something to apply, from any of them. */
function ApplyBar({ s, t }: { s: SyncState; t: ThemeState }) {
  const summary = [
    t.add.length ? `Installs ${names(t.add)}.` : '',
    t.remove.length ? `Removes ${names(t.remove)}.` : '',
    t.set.length
      ? `Sets ${shortList(
          t.set.map(([id, v]) => {
            const x = SETTINGS[id];
            return `${x.name} to ${new Intl.NumberFormat(undefined, x.format).format(v)}${x.unit ?? ''}`;
          }),
          2,
        )}.`
      : '',
  ]
    .filter(Boolean)
    .join(' ');
  return (
    <div className="apply-bar">
      <p className="detail">{summary}</p>
      <Link disabled={!s.idle} onPress={t.revert}>
        Revert
      </Link>
      <Push prominent disabled={!s.idle} onPress={t.apply}>
        {t.theme === null ? 'Install' : 'Apply'}
      </Push>
    </div>
  );
}
