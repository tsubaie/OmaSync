import { describe, expect, it } from 'vitest';
import { ago, byFolder, plural, progress, shortList } from './format';

describe('format', () => {
  it('says time and counts in plain words', () => {
    expect(plural(1, 'app')).toBe('1 app');
    expect(plural(3, 'app')).toBe('3 apps');
    expect(ago(970, 1000)).toBe('just now');
    expect(ago(940, 1000)).toBe('1 minute ago');
    expect(ago(0, 7200)).toBe('2 hours ago');
    expect(ago(0, 86400 * 3)).toBe('3 days ago');
  });

  it('groups paths under the most specific folder name', () => {
    const catalog = {
      data_label: 'Apps and sync choices',
      folders: [
        { path: '.config/hypr', name: 'Windows', default: true, exists: true },
        { path: '.config', name: 'Everything', default: false, exists: true },
      ],
    };
    expect(
      byFolder(['.config/hypr/a.lua', 'Apps and sync choices', '.config/hyprx/b'], catalog),
    ).toEqual([
      ['Apps and sync choices', []],
      ['Everything', ['b']],
      ['Windows', ['a.lua']],
    ]);
    expect(shortList(['a', 'b', 'c'], 2)).toBe('a, b and 1 more');
  });
});

describe('progress', () => {
  it('lists reached steps, then the rest of the plan, and counts the percentage', () => {
    const plan = ['Save', 'Fetch', 'Share'];
    expect(progress(['Save', 'Saving 3 new files', 'Fetch'], plan, 50)).toEqual({
      items: ['Save', 'Saving 3 new files', 'Fetch', 'Share'],
      share: 2.5 / 4,
    });
    expect(progress([], plan, null).share).toBe(0);
  });
});
