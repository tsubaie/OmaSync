import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import type { BackupEntry, Conflict, SetupState } from './api';
import { diffLines, folds } from './format';
import { OmarchyPane } from './panes/FoldersPane';
import { HistoryPane } from './panes/HistoryPane';
import { Setup } from './panes/Setup';
import { SyncPane } from './panes/SyncPane';
import type { SyncState } from './state';

const catalog = {
  data_label: 'Apps and sync choices',
  folders: [
    { path: '.config/hypr', name: 'Other window settings', default: true, exists: true, app: null },
    { path: '.config/kitty', name: 'Kitty terminal', default: true, exists: true, app: 'kitty' },
    { path: '.config/nvim', name: 'Neovim editor', default: false, exists: true, app: 'neovim' },
  ],
};

const setup = (st: Partial<SetupState>, signingIn = false) =>
  renderToStaticMarkup(
    <Setup
      s={{ idle: true, catalog, signingIn, steps: [], plan: [] } as unknown as SyncState}
      st={{
        missing: [],
        signed_in: true,
        login: 'someone',
        found: false,
        ready: false,
        // Kitty isn't installed here, so it isn't on by default after all.
        folders: [
          { path: '.config/hypr', default: true },
          { path: '.config/kitty', default: false },
          { path: '.config/nvim', default: false },
        ],
        apps: [],
        plugins: [],
        ...st,
      }}
    />,
  );

/** A file of `n` lines, with `text` at line `at`. */
const lines = (n: number, at: number, text: string) =>
  Array.from({ length: n }, (_, i) => (i === at ? text : `line ${i}`)).join('\n');

const clash = (conflict: Conflict) =>
  renderToStaticMarkup(
    <SyncPane
      s={
        {
          idle: true,
          catalog,
          conflict,
          now: 1000,
          steps: [],
          plan: [],
          nameOf: () => 'Desktop',
          overview: { computers: [], history: [] },
        } as unknown as SyncState
      }
    />,
  );

const version = (path: string) => ({
  path,
  source: path,
  mine: lines(30, 15, 'mine'),
  theirs: lines(30, 15, 'theirs'),
  binary: false,
});

const history = (backups: BackupEntry[]) =>
  renderToStaticMarkup(
    <HistoryPane
      s={
        {
          idle: true,
          catalog,
          now: 100_000,
          overview: { history: [], backups },
        } as unknown as SyncState
      }
    />,
  );

const folders = (computers: number) =>
  renderToStaticMarkup(
    <OmarchyPane
      s={
        {
          idle: true,
          host: 'me',
          catalog,
          data: { folders: ['.config/hypr'], apps: [], computers: {} },
          overview: {
            apps: { installed: [] },
            computers: Array.from({ length: computers }, (_, i) => [`c${i}`, null]),
            plugins: [],
          },
        } as unknown as SyncState
      }
    />,
  );

describe('step 3 of setup', () => {
  it('names only what the engine will sync by default, and offers a choice', () => {
    const html = setup({});
    expect(html).toContain('Your other window settings.');
    expect(html).not.toContain('kitty terminal');
    expect(html).toContain('Choose What Syncs…');
    expect(html).toContain('aria-expanded="false"');
  });

  it('offers the choice to a joining computer too', () => {
    expect(setup({ found: true })).toContain('Choose What Syncs…');
  });

  it('explains the code and the browser while signing in', () => {
    const html = setup({ signed_in: false }, true);
    expect(html).toContain('one-time code');
    expect(html).toContain('moves on by itself');
    expect(html).toContain('I&#x27;ve Signed In');
  });
});

describe('a long clashing file', () => {
  it('folds unchanged runs, keeping 3 lines of context around each change', () => {
    const rows = diffLines(lines(30, 15, 'mine'), lines(30, 15, 'theirs'));
    expect(folds(rows)).toEqual([
      [0, 12],
      [19, 30],
    ]);
    // Short runs stay as they are.
    expect(folds(diffLines(lines(7, 3, 'a'), lines(7, 3, 'b')))).toEqual([]);
  });

  it('shows a fold and a choice for all files at once', () => {
    const html = clash({
      files: [],
      by: ['desk', 900],
      broken: [],
      versions: [version('.config/hypr/a'), version('.config/hypr/b')],
    });
    expect(html).toContain('Show 12 unchanged lines');
    expect(html).toContain('Use This Computer for All');
    expect(html).toContain('Use Desktop for All');
  });

  it('shows no empty card, and Sync Now instead', () => {
    const html = clash({ files: [], by: null, broken: [], versions: [] });
    expect(html).not.toContain('Changed on Both Computers');
    expect(html).toContain('Sync Now');
  });
});

describe('safety backups', () => {
  it('are listed with what they hold, and restored on request', () => {
    const html = history([
      {
        name: 'before-syncing-1.tar.gz',
        label: 'Before syncing',
        at: 100_000 - 7200,
        folders: ['.config/hypr'],
      },
    ]);
    expect(html).toContain('Safety Backups');
    expect(html).toContain('Before syncing');
    expect(html).toContain('2 hours ago: Other window settings');
    expect(html).toContain('Restore…');
  });

  it('are left out when there are none', () => {
    expect(history([])).not.toContain('Safety Backups');
  });
});

describe('a folder switch', () => {
  it('says it reaches every computer, beside the switch for this one only', () => {
    const html = folders(2);
    expect(html).toContain('aria-label="Sync Other window settings (all computers)"');
    expect(html).toContain('Don&#x27;t Sync on This Computer');
    expect(html).toContain('On for all your computers');
  });

  it('stays simple with one computer', () => {
    const html = folders(1);
    expect(html).toContain('aria-label="Sync Other window settings"');
    expect(html).not.toContain('On for all your computers');
    expect(html).not.toContain('>Don&#x27;t Sync on This Computer<');
  });
});
