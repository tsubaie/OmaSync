import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import type { MissingRepo, Snapshot } from './api';
import { NoticeLine } from './panes/NoticeLine';
import { ReposPane } from './panes/ReposPane';
import { type SyncState, snapshotNotice } from './state';

const repo: MissingRepo = {
  name: 'chaotic-aur',
  repository: { servers: ['https://example.com/chaotic'], sig_level: [] },
  apps: ['yay-bin'],
};

const repos = (snapper_available: boolean) =>
  renderToStaticMarkup(
    <ReposPane
      s={
        {
          idle: true,
          overview: { missing_repos: [repo], repos_here: [], snapper_available },
        } as unknown as SyncState
      }
    />,
  );

const notice = (snapshot: Snapshot | null) =>
  renderToStaticMarkup(
    <NoticeLine
      s={{ notice: snapshotNotice('Installed firefox.', snapshot) } as unknown as SyncState}
    />,
  );

describe('a snapshot of the system', () => {
  it('is promised before adding a repository only where Snapper can take one', () => {
    expect(repos(true)).toContain('A snapshot of this computer');
    expect(repos(true)).toContain('go back to it from the startup menu');
    expect(repos(false)).not.toContain('snapshot');
  });

  it('is named by its number afterwards', () => {
    const html = notice({ Taken: 42 });
    expect(html).toContain('Installed firefox. Snapshot 42 of this computer');
    expect(html).toContain('restart and pick it from the startup menu');
    expect(html).toContain('notice good');
  });

  it('shows as a warning when it failed, and says nothing more without Snapper', () => {
    const html = notice({ Failed: 'snapper: no space' });
    expect(html).toContain('notice warn');
    expect(html).toContain('A snapshot couldn');
    expect(notice('Unavailable')).toContain('notice good');
    expect(notice(null)).not.toContain('napshot');
  });
});
