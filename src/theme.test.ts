import { readdirSync, readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { contrast, hex, palette } from './theme';

describe('palette', () => {
  it('reads the theme and falls back to Catppuccin', () => {
    // Selection and buttons use Apple's blue whatever the theme's accent, with white text on it.
    const p = palette('accent = "#ff0000"\n');
    expect(p.accent).toEqual(hex('#0058d0'));
    expect(palette('background = "#eff1f5"\nforeground = "#4c4f69"\n').accent).toEqual(
      hex('#0064e1'),
    );
    expect(p['on-accent']).toEqual([1, 1, 1]);
    expect(p.bg).toEqual(hex('#1e1e2e'));
    expect(hex('#12345')).toBeNull();
    expect(contrast([0, 0, 0], [1, 1, 1])).toBeCloseTo(21, 1);
  });

  // Only present on Omarchy machines; CI checks the fallback.
  const dir = '/usr/share/omarchy/themes';
  let themes: [string, string][] = [['fallback', '']];
  try {
    themes = themes.concat(
      readdirSync(dir).flatMap((t) => {
        try {
          return [[t, readFileSync(`${dir}/${t}/colors.toml`, 'utf8')] as [string, string]];
        } catch {
          return [];
        }
      }),
    );
  } catch {
    // Not an Omarchy machine.
  }

  it.each(themes)('%s passes WCAG contrast for every role', (_name, toml) => {
    const p = palette(toml);
    const pairs: [string, string, number][] = [
      ['ink', 'card', 4.5],
      ['quiet', 'bg', 4.5],
      ['quiet', 'card', 4.5],
      ['ink', 'sidebar', 4.5],
      ['quiet', 'sidebar', 4.5],
      ['warn', 'card', 4.5],
      ['on-accent', 'accent', 4.5],
      ['on-knob', 'accent', 3],
      ['accent-mark', 'bg', 3],
      ['off-edge', 'bg', 3],
      ['off-knob', 'raised', 3],
      ['on-tile-apps', 'tile-apps', 3],
      ['on-tile-history', 'tile-history', 3],
      ['on-tile-settings', 'tile-settings', 3],
    ];
    for (const [role, on, min] of pairs) {
      expect(contrast(p[role], p[on]), `${role} on ${on}`).toBeGreaterThanOrEqual(min);
    }
  });
});
