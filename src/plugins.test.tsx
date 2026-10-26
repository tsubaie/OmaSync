import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import type { Plugin } from './api';
import { NoticeLine } from './panes/NoticeLine';
import { PluginsPane } from './panes/FoldersPane';
import type { SyncState } from './state';

const plugin = (id: string, update: Plugin['update']): Plugin => ({
  id,
  name: id,
  description: '',
  kinds: [],
  first_party: false,
  here: true,
  enabled: true,
  can_disable: true,
  source: `https://example.com/${id}.git`,
  update,
});

const pane = (plugins: Plugin[]) =>
  renderToStaticMarkup(
    <PluginsPane
      s={
        {
          idle: true,
          host: 'me',
          plan: [],
          steps: [],
          catalog: { folders: [] },
          data: { folders: ['.config/omarchy/plugins'], apps: [], plugins: [], computers: {} },
          overview: { plugins, apps: { installed: [] } },
          matrix: { computers: ['me'], name: () => 'This computer', column: () => '', many: false },
          wantsPlugin: () => false,
          whoHas: () => [],
        } as unknown as SyncState
      }
    />,
  );

describe('a plugin from git', () => {
  it('offers Update only when its source has something newer', () => {
    const html = pane([plugin('newer', 'newer'), plugin('current', 'current')]);
    expect(html).toContain('Update (an update is available for newer)');
    expect(html).not.toContain('Update (current)');
    expect(html).toContain('Up to date');
    expect(html).toContain('1 update available.');
  });

  it('says it was changed on this computer, and offers no Update', () => {
    const html = pane([plugin('mine', 'edited')]);
    expect(html).toContain('Changed on this computer');
    expect(html).not.toContain('Update (');
  });

  it('tells how to keep a plugin when it is removed elsewhere', () => {
    expect(pane([])).toContain('to keep it on one as its own, switch Sync off there first');
  });
});

const notice = (warn: boolean) =>
  renderToStaticMarkup(
    <NoticeLine
      s={{ notice: { good: true, warn, text: 'Updated 2 plugins.' } } as unknown as SyncState}
    />,
  );

describe('the summary of Update All', () => {
  it('is a warning when a plugin was skipped or failed', () => {
    expect(notice(true)).toContain('notice warn');
    expect(notice(false)).toContain('notice good');
  });
});
