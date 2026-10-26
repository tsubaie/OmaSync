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

const pane = (plugins: Plugin[], s: Partial<SyncState> = {}) =>
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
          overview: { plugins, apps: { installed: [] }, computers: [] },
          wantsPlugin: () => false,
          whoHas: () => [],
          pluginsWaiting: [],
          ...s,
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
    expect(pane([])).toContain(
      'to keep it on one as its own, choose Don&#x27;t Sync on This Computer there first',
    );
  });
});

describe('your plugins', () => {
  const two = {
    data: {
      folders: ['.config/omarchy/plugins'],
      apps: [],
      plugins: ['dock', 'webcam'],
      computers: {},
    },
    overview: {
      plugins: [],
      apps: { installed: [] },
      computers: [
        ['me', 1],
        ['desk', 1],
      ],
    },
  } as unknown as Partial<SyncState>;
  const here = plugin('dock', null);
  const waiting = { ...plugin('webcam', null), here: false, enabled: null };
  const html = (wants: boolean) =>
    pane([here, waiting], {
      ...two,
      overview: { ...two.overview!, plugins: [here, waiting] },
      wantsPlugin: () => wants,
      pluginsWaiting: wants ? [waiting] : [],
    });

  it('lists those not on this computer apart, each with Install', () => {
    const out = html(true);
    const [mine, missing] = out.split('<h2>Not Installed on This Computer</h2>');
    expect(mine).toContain('aria-label="Sync dock (all computers)"');
    expect(mine).not.toContain('webcam');
    expect(missing).toContain('aria-label="Sync webcam (all computers)"');
    expect(missing).toContain('>Install<');
  });

  it('has one switch each, and leaves this computer out like Configurations', () => {
    const out = html(true);
    expect(out.match(/role="switch"/g)).toHaveLength(2);
    expect(out).not.toContain('Enabled');
    expect(out).toContain('>Don&#x27;t Sync on This Computer</button>');
    const skipped = html(false);
    expect(skipped).toContain('>Sync on This Computer</button>');
    expect(skipped).not.toContain('>Don&#x27;t Sync on This Computer</button>');
    expect(skipped).not.toContain('>Install<');
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
