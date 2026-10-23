import { Check, Package, RefreshCw } from 'lucide-react';
import { api, type SetupState } from '../api';
import type { SyncState } from '../state';
import { Group, Link, Push, Row, Tile } from '../ui';
import { NoticeLine } from './NoticeLine';

/** What step 1 installs, named so nobody has to trust an unnamed "helper". */
const TOOLS: Record<string, [string, string]> = {
  chezmoi: ['chezmoi', 'Copies your settings files to and from your private sync space.'],
  'github-cli': [
    'GitHub CLI',
    'Connects to your GitHub account to create and reach your private sync space.',
  ],
  git: ['Git', 'Keeps every version of your settings, which is what makes Undo possible.'],
};

function action(s: SyncState, st: SetupState): [string, () => void] {
  const install = () => s.act('Installing', () => api.installHelpers(), s.probe);
  if (st.missing.length === 1)
    return [`Install ${TOOLS[st.missing[0]]?.[0] ?? st.missing[0]}`, install];
  if (st.missing.length > 1) return [`Install ${st.missing.length} Tools`, install];
  if (!st.signed_in && s.signingIn) return ["I've Signed In", s.probe];
  if (!st.signed_in)
    return ['Sign In to GitHub', () => api.signIn().then(() => s.setSigningIn(true), s.fail)];
  if (st.found) {
    return [
      'Use My Synced Settings',
      () =>
        s.act(
          'Getting ready',
          () => api.join(() => {}),
          async () => {
            await s.probe();
            await s.sync();
          },
        ),
    ];
  }
  return [
    'Start Syncing',
    () =>
      s.act(
        'Getting ready',
        () => api.start(() => {}),
        async () => {
          await s.probe();
          await s.sync();
        },
      ),
  ];
}

export function Setup({ s, st }: { s: SyncState; st: SetupState }) {
  const current = st.missing.length ? 0 : !st.signed_in ? 1 : 2;
  const steps = [
    'Install the tools it needs',
    'Sign in to GitHub',
    st.found ? 'Bring in your synced settings' : "Save this computer's settings",
  ];
  // Lowercase mid-sentence, except the product name.
  const what = s.catalog.folders
    .filter((f) => f.default && f.exists)
    .map((f) => (f.name.startsWith('Omarchy') ? f.name : f.name.toLowerCase()));
  const plan: [string, string][] = st.found
    ? [
        [
          "Backs up this computer's settings first",
          'To ~/.local/share/omarchy-sync/backups, so you can get them back.',
        ],
        [
          'Replaces them with your synced settings',
          'From the private omarchy-sync space in your GitHub account.',
        ],
        [
          'Lets you approve apps before installing',
          'Synced apps wait on the Sync pane; nothing installs on its own.',
        ],
        ['Syncs every 15 minutes from now on', 'You can turn this off any time on the Sync pane.'],
      ]
    : [
        [
          'Creates a private space called omarchy-sync',
          'In your GitHub account. Only you can see it.',
        ],
        ["Copies this computer's settings into it", `Your ${what.join(', ')}.`],
        ['Changes nothing on this computer', 'It only reads your settings.'],
        ['Syncs every 15 minutes from now on', 'You can turn this off any time on the Sync pane.'],
      ];
  // Each step says why it's needed, not just what it does.
  const why = st.missing.length
    ? `Installing software needs your password, so Omarchy will ask for it once. Nothing else on your computer changes, and you can remove ${st.missing.length === 1 ? 'it' : 'them'} later like any other app.`
    : !st.signed_in && s.signingIn
      ? 'Finish signing in in the window that opened, then come back here.'
      : !st.signed_in
        ? 'Your settings are stored in a private space in your GitHub account that only you can see. Signing in happens in your browser.'
        : st.found
          ? "We found settings from your other computers. Here's what happens when you use them:"
          : "This is your first computer, so its settings become the starting point for the others. Here's what happens when you start:";
  const [label, go] = action(s, st);

  return (
    <div className="setup">
      <div className="intro">
        <Tile icon={RefreshCw} kind="sync" size={64} />
        <h1>Keep Your Computers in Step</h1>
        <p>Your Omarchy settings, themes, plugins and apps, the same on every computer.</p>
        <p className="sub">Step {current + 1} of 3</p>
      </div>
      <Group>
        {steps.map((step, i) => (
          <div className="row" key={step}>
            <span
              className={`badge ${i < current ? 'done' : i === current ? 'now' : ''}`}
              aria-hidden="true"
            >
              {i < current ? <Check size={13} strokeWidth={3} /> : i + 1}
            </span>
            <span className={`step ${i === current ? 'now' : ''}`}>
              {step}
              {i < current && <span className="visually-hidden"> (done)</span>}
            </span>
          </div>
        ))}
      </Group>
      {s.idle && st.missing.length > 0 && (
        <Group>
          {st.missing.map((pkg) => (
            <Row
              key={pkg}
              lead={<Tile icon={Package} kind="apps" size={26} />}
              title={TOOLS[pkg]?.[0] ?? pkg}
              detail={`${TOOLS[pkg]?.[1] ?? ''} From Arch Linux's official packages.`}
            />
          ))}
        </Group>
      )}
      <p>{why}</p>
      {s.idle && st.signed_in && (
        <Group>
          {plan.map(([title, detail]) => (
            <Row key={title} title={title} detail={detail} />
          ))}
        </Group>
      )}
      <div className="actions">
        {s.busy ? (
          <output className="loading">
            <span className="dot" />
            {s.busy}…
          </output>
        ) : (
          <>
            {s.signingIn && !st.signed_in && (
              <Link onPress={() => api.signIn().catch(s.fail)}>Open the Sign-In Window Again</Link>
            )}
            <Push prominent onPress={go}>
              {label}
            </Push>
          </>
        )}
      </div>
      <NoticeLine s={s} />
    </div>
  );
}
