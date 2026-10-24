// Colours come from the active Omarchy theme, then get nudged until they pass WCAG contrast,
// because any of Omarchy's themes (or a user's own) can be the input. They're applied as CSS
// custom properties on <html>.

import type { Look } from './api';

type Rgb = [number, number, number];

const WHITE: Rgb = [1, 1, 1];
const BLACK: Rgb = [0, 0, 0];

export function hex(s: string): Rgb | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(s.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

const css = (c: Rgb) =>
  `#${c
    .map((v) =>
      Math.round(Math.min(1, Math.max(0, v)) * 255)
        .toString(16)
        .padStart(2, '0'),
    )
    .join('')}`;

export const mix = (a: Rgb, b: Rgb, t: number): Rgb => [
  a[0] + (b[0] - a[0]) * t,
  a[1] + (b[1] - a[1]) * t,
  a[2] + (b[2] - a[2]) * t,
];

const lin = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);

function luminance(c: Rgb) {
  return 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]);
}

/** WCAG contrast ratio, 1 to 21. */
export function contrast(a: Rgb, b: Rgb) {
  const [x, y] = [luminance(a), luminance(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}

/** Moves `c` toward `toward` in small steps until it reaches `ratio` against every colour in `on`. */
function ensure(c: Rgb, on: Rgb[], ratio: number, toward: Rgb): Rgb {
  for (let i = 0; i <= 20; i++) {
    const candidate = mix(c, toward, i / 20);
    if (on.every((b) => contrast(candidate, b) >= ratio)) return candidate;
  }
  return toward;
}

/** Whichever of white, black or the theme's text/background reads best on `c`. */
const bestOn = (c: Rgb, bg: Rgb, ink: Rgb) =>
  [WHITE, bg, ink, BLACK].reduce((best, x) => (contrast(x, c) > contrast(best, c) ? x : best));

/** Named colour roles, each already contrast-checked. Falls back to Omarchy's Catppuccin. */
export function palette(toml: string): Record<string, Rgb> {
  const get = (key: string, fallback: string): Rgb => {
    const line = toml.split('\n').find((l) => l.split('=')[0]?.trim() === key);
    return (line && hex(line.split('=')[1].trim().replace(/"/g, ''))) || hex(fallback)!;
  };
  const bg = get('background', '#1e1e2e');
  const ink = get('foreground', '#cdd6f4');
  const raised = get('lighter_background', '#313244');
  // macOS designs each appearance on its own: light windows put white cards on a soft grey,
  // dark ones lift cards a step lighter than the window.
  const light = luminance(bg) > 0.4;
  // Selection, buttons and switches use the Omarchy theme's own accent, so the app matches the
  // desktop; the text on it is whichever reads best (checked for every bundled theme).
  const accent = get('accent', '#89b4fa');
  const card = light ? mix(bg, WHITE, 0.75) : mix(bg, ink, 0.06);
  // The sidebar: the theme's own darker shade, a step below the window.
  const sidebar = get('dark_background', '#161622');
  const surfaces = [bg, card, sidebar];
  const onAccent = bestOn(accent, bg, ink);
  const p: Record<string, Rgb> = {
    bg,
    card,
    sidebar,
    line: mix(bg, ink, 0.12),
    raised,
    ink,
    quiet: ensure(get('dark_foreground', '#6c7086'), surfaces, 4.5, ink),
    accent,
    'accent-mark': ensure(accent, surfaces, 3, ink),
    'on-accent': onAccent,
    'on-knob': contrast(bg, accent) >= 3 ? bg : onAccent,
    good: ensure(get('green', '#a6e3a1'), surfaces, 3, ink),
    warn: ensure(get('yellow', '#f9e2af'), surfaces, 4.5, ink),
    bad: ensure(get('red', '#f38ba8'), surfaces, 4.5, ink),
    'off-edge': ensure(mix(raised, ink, 0.3), surfaces, 3, ink),
    'off-knob': ensure(mix(raised, ink, 0.5), [raised], 3, ink),
  };
  // Section tiles: Sync uses cyan, not the accent, so it stays visible on the selected (accent) row.
  const tiles: [string, string, string][] = [
    ['sync', 'cyan', '#94e2d5'],
    ['apps', 'orange', '#fab387'],
    ['folders', 'green', '#a6e3a1'],
    ['history', 'magenta', '#f5c2e7'],
  ];
  for (const [name, key, fallback] of tiles) {
    p[`tile-${name}`] = get(key, fallback);
    p[`on-tile-${name}`] = bestOn(p[`tile-${name}`], bg, ink);
  }
  // Settings is grey, as in macOS.
  p['tile-settings'] = mix(raised, ink, 0.3);
  p['on-tile-settings'] = bestOn(p['tile-settings'], bg, ink);
  return p;
}

/** Dresses the page like the desktop: theme colours, light or dark, and motion. */
export function applyLook(look: Look) {
  const root = document.documentElement;
  const p = palette(look.colors_toml);
  for (const [name, value] of Object.entries(p)) root.style.setProperty(`--${name}`, css(value));
  root.style.colorScheme = luminance(p.bg) < 0.2 ? 'dark' : 'light';
  root.classList.toggle('no-motion', !look.motion);
}
